//! Zero-knowledge chunked file & document stream transfer (Issue #2370 / WAVE-26.08)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

#[derive(Debug, Error)]
pub enum FileTransferError {
    #[error("Checksum mismatch: expected {expected}, actual {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Chunk index out of bounds: {index} >= {total}")]
    ChunkOutOfBounds { index: usize, total: usize },

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Incomplete transfer: received {received}/{total} chunks")]
    IncompleteTransfer { received: usize, total: usize },

    #[error("Transfer I/O error: {0}")]
    IoError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub session_id: String,
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub data: Vec<u8>,
    pub chunk_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub session_id: String,
    pub file_name: String,
    pub total_bytes: usize,
    pub chunk_size: usize,
    pub total_chunks: usize,
    pub file_sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub session_id: String,
    pub received_chunks: usize,
    pub total_chunks: usize,
    pub bytes_transferred: usize,
    pub is_completed: bool,
}

pub struct FileTransferSession {
    pub manifest: ChunkManifest,
    pub chunks: HashMap<usize, Vec<u8>>,
    pub is_finalized: bool,
}

impl FileTransferSession {
    pub fn new(manifest: ChunkManifest) -> Self {
        Self {
            manifest,
            chunks: HashMap::new(),
            is_finalized: false,
        }
    }

    pub fn insert_chunk(&mut self, chunk: FileChunk) -> Result<(), FileTransferError> {
        if chunk.chunk_index >= self.manifest.total_chunks {
            return Err(FileTransferError::ChunkOutOfBounds {
                index: chunk.chunk_index,
                total: self.manifest.total_chunks,
            });
        }

        // Validate chunk hash
        let mut hasher = Sha256::new();
        hasher.update(&chunk.data);
        let actual_hash = to_hex(&hasher.finalize());
        if actual_hash != chunk.chunk_sha256 {
            return Err(FileTransferError::ChecksumMismatch {
                expected: chunk.chunk_sha256,
                actual: actual_hash,
            });
        }

        self.chunks.insert(chunk.chunk_index, chunk.data);
        Ok(())
    }

    pub fn progress(&self) -> TransferProgress {
        let received = self.chunks.len();
        let bytes_transferred: usize = self.chunks.values().map(|c| c.len()).sum();
        TransferProgress {
            session_id: self.manifest.session_id.clone(),
            received_chunks: received,
            total_chunks: self.manifest.total_chunks,
            bytes_transferred,
            is_completed: received == self.manifest.total_chunks,
        }
    }

    pub fn assemble(&mut self) -> Result<Vec<u8>, FileTransferError> {
        if self.chunks.len() != self.manifest.total_chunks {
            return Err(FileTransferError::IncompleteTransfer {
                received: self.chunks.len(),
                total: self.manifest.total_chunks,
            });
        }

        let mut assembled = Vec::with_capacity(self.manifest.total_bytes);
        for i in 0..self.manifest.total_chunks {
            if let Some(part) = self.chunks.get(&i) {
                assembled.extend_from_slice(part);
            } else {
                return Err(FileTransferError::IncompleteTransfer {
                    received: self.chunks.len(),
                    total: self.manifest.total_chunks,
                });
            }
        }

        // Verify total file checksum
        let mut hasher = Sha256::new();
        hasher.update(&assembled);
        let actual_hash = to_hex(&hasher.finalize());
        if actual_hash != self.manifest.file_sha256 {
            return Err(FileTransferError::ChecksumMismatch {
                expected: self.manifest.file_sha256.clone(),
                actual: actual_hash,
            });
        }

        self.is_finalized = true;
        Ok(assembled)
    }
}

pub struct FileTransferManager {
    sessions: Arc<RwLock<HashMap<String, FileTransferSession>>>,
}

impl FileTransferManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn init_transfer(&self, manifest: ChunkManifest) {
        let mut lock = self.sessions.write().await;
        lock.insert(
            manifest.session_id.clone(),
            FileTransferSession::new(manifest),
        );
    }

    pub async fn assemble_chunk(
        &self,
        chunk: FileChunk,
    ) -> Result<TransferProgress, FileTransferError> {
        let mut lock = self.sessions.write().await;
        let session = lock
            .get_mut(&chunk.session_id)
            .ok_or_else(|| FileTransferError::SessionNotFound(chunk.session_id.clone()))?;

        session.insert_chunk(chunk)?;
        Ok(session.progress())
    }

    pub async fn resume_transfer(&self, session_id: &str) -> Result<Vec<usize>, FileTransferError> {
        let lock = self.sessions.read().await;
        let session = lock
            .get(session_id)
            .ok_or_else(|| FileTransferError::SessionNotFound(session_id.to_string()))?;

        let mut missing = Vec::new();
        for i in 0..session.manifest.total_chunks {
            if !session.chunks.contains_key(&i) {
                missing.push(i);
            }
        }
        Ok(missing)
    }

    pub async fn finalize_transfer(&self, session_id: &str) -> Result<Vec<u8>, FileTransferError> {
        let mut lock = self.sessions.write().await;
        let session = lock
            .get_mut(session_id)
            .ok_or_else(|| FileTransferError::SessionNotFound(session_id.to_string()))?;

        session.assemble()
    }
}

impl Default for FileTransferManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn verify_file_checksum(data: &[u8], expected_sha256: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = to_hex(&hasher.finalize());
    actual.eq_ignore_ascii_case(expected_sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_and_chunked_transfer() {
        let manager = FileTransferManager::new();
        let payload = b"Hello, encrypted chunked file mesh in Xavier!";

        let mut hasher = Sha256::new();
        hasher.update(payload);
        let full_hash = to_hex(&hasher.finalize());

        let chunk_size = 16;
        let total_chunks = payload.len().div_ceil(chunk_size);

        let manifest = ChunkManifest {
            session_id: "sess_001".into(),
            file_name: "test.txt".into(),
            total_bytes: payload.len(),
            chunk_size,
            total_chunks,
            file_sha256: full_hash.clone(),
            created_at: Utc::now(),
        };

        manager.init_transfer(manifest).await;

        for (i, slice) in payload.chunks(chunk_size).enumerate() {
            let mut c_hasher = Sha256::new();
            c_hasher.update(slice);
            let c_hash = to_hex(&c_hasher.finalize());

            let chunk = FileChunk {
                session_id: "sess_001".into(),
                chunk_index: i,
                total_chunks,
                data: slice.to_vec(),
                chunk_sha256: c_hash,
            };

            let progress = manager.assemble_chunk(chunk).await.unwrap();
            assert_eq!(progress.received_chunks, i + 1);
        }

        let assembled = manager.finalize_transfer("sess_001").await.unwrap();
        assert_eq!(assembled, payload);
    }

    #[tokio::test]
    async fn test_corrupt_chunk_rejected() {
        let manager = FileTransferManager::new();
        let manifest = ChunkManifest {
            session_id: "sess_corrupt".into(),
            file_name: "bad.bin".into(),
            total_bytes: 10,
            chunk_size: 10,
            total_chunks: 1,
            file_sha256: "somehash".into(),
            created_at: Utc::now(),
        };
        manager.init_transfer(manifest).await;

        let bad_chunk = FileChunk {
            session_id: "sess_corrupt".into(),
            chunk_index: 0,
            total_chunks: 1,
            data: b"some data".to_vec(),
            chunk_sha256: "invalid_hash_value".into(),
        };

        let res = manager.assemble_chunk(bad_chunk).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_resume_transfer_missing_chunks() {
        let manager = FileTransferManager::new();
        let manifest = ChunkManifest {
            session_id: "sess_resume".into(),
            file_name: "resume.dat".into(),
            total_bytes: 30,
            chunk_size: 10,
            total_chunks: 3,
            file_sha256: "placeholder".into(),
            created_at: Utc::now(),
        };
        manager.init_transfer(manifest).await;

        let missing = manager.resume_transfer("sess_resume").await.unwrap();
        assert_eq!(missing, vec![0, 1, 2]);

        // Insert chunk 1
        let slice = b"1234567890";
        let mut c_hasher = Sha256::new();
        c_hasher.update(slice);
        let c_hash = to_hex(&c_hasher.finalize());

        let chunk = FileChunk {
            session_id: "sess_resume".into(),
            chunk_index: 1,
            total_chunks: 3,
            data: slice.to_vec(),
            chunk_sha256: c_hash,
        };

        manager.assemble_chunk(chunk).await.unwrap();
        let missing_after = manager.resume_transfer("sess_resume").await.unwrap();
        assert_eq!(missing_after, vec![0, 2]);
    }
}
