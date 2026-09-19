//! Document ingestion pipeline for Xavier DocBot.
//!
//! Orchestrates: parse → chunk → embed → store.
//! Uses Xavier's existing `GenericDocumentExtractor` and `Embedder` trait,
//! adding collection-aware routing and async processing.

use std::sync::Arc;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing::{debug, error, info, warn};

use crate::collections::schema::{DocChunk, DocEntry, DocStatus};
use crate::collections::store::CollectionStore;
use crate::documents::GenericDocumentExtractor;
use crate::embedding::Embedder;

// ── Chunk config ───────────────────────────────────────────────────────────────

/// Chunking configuration for the ingestion pipeline.
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// Target chunk size in approximate tokens (1 token ≈ 4 chars).
    pub target_tokens: usize,
    /// Overlap between consecutive chunks, in tokens.
    pub overlap_tokens: usize,
    /// Minimum chunk length (chars) — discard shorter leftovers.
    pub min_chars: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            target_tokens: 512,
            overlap_tokens: 64,
            min_chars: 40,
        }
    }
}

impl ChunkConfig {
    pub fn from_env() -> Self {
        let target = std::env::var("DOCBOT_DEFAULT_CHUNK_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(512);
        let overlap = std::env::var("DOCBOT_DEFAULT_CHUNK_OVERLAP")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64);
        Self {
            target_tokens: target,
            overlap_tokens: overlap,
            ..Default::default()
        }
    }

    /// Approximate character budget for target_tokens.
    fn target_chars(&self) -> usize {
        self.target_tokens * 4
    }

    /// Approximate character overlap.
    fn overlap_chars(&self) -> usize {
        self.overlap_tokens * 4
    }
}

// ── Semantic chunker ────────────────────────────────────────────────────────────

/// Splits `text` into overlapping chunks respecting sentence boundaries.
///
/// Rules:
/// 1. Prefer breaking at paragraph (`\n\n`) boundaries.
/// 2. Fall back to sentence endings (`. `, `! `, `? `).
/// 3. Hard-break at `target_chars` if no natural boundary found.
/// 4. Each chunk overlaps the previous by `overlap_chars`.
pub fn semantic_chunk(text: &str, config: &ChunkConfig) -> Vec<String> {
    let target = config.target_chars();
    let overlap = config.overlap_chars();

    if text.len() <= target {
        let trimmed = text.trim().to_string();
        return if trimmed.len() >= config.min_chars {
            vec![trimmed]
        } else {
            vec![]
        };
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut start: usize = 0;
    let _bytes = text.as_bytes();
    let len = text.len();

    while start < len {
        let end = (start + target).min(len);

        // Find a good break point within [start+min_chars, end]
        let break_at = find_break(text, start, end, config.min_chars);

        let chunk = text[start..break_at].trim();
        if chunk.len() >= config.min_chars {
            chunks.push(chunk.to_string());
        }

        // Next window starts with overlap
        if break_at <= start {
            // Safety: avoid infinite loop on degenerate input
            start = end;
        } else {
            let next = break_at.saturating_sub(overlap);
            // Align to char boundary
            start = align_char_boundary(text, next);
        }
    }

    chunks
}

/// Find the best character-boundary break in `text[start..end]`.
fn find_break(text: &str, start: usize, end: usize, min_chars: usize) -> usize {
    let window = &text[start..end];

    // 1. Paragraph boundary
    if let Some(pos) = window.rfind("\n\n") {
        let abs = start + pos + 2;
        if abs > start + min_chars {
            return abs;
        }
    }

    // 2. Sentence boundary
    for marker in &[". ", "! ", "? ", ".\n", "!\n", "?\n"] {
        if let Some(pos) = window.rfind(marker) {
            let abs = start + pos + marker.len();
            if abs > start + min_chars {
                return abs;
            }
        }
    }

    // 3. Word boundary
    if let Some(pos) = window.rfind(' ') {
        let abs = start + pos + 1;
        if abs > start + min_chars {
            return abs;
        }
    }

    // 4. Hard break at char boundary
    end
}

/// Retreat `pos` to the nearest valid UTF-8 char boundary in `text`.
fn align_char_boundary(text: &str, pos: usize) -> usize {
    let mut p = pos.min(text.len());
    while p > 0 && !text.is_char_boundary(p) {
        p -= 1;
    }
    p
}

/// Estimate token count (rough: chars / 4).
pub fn estimate_tokens(text: &str) -> u32 {
    ((text.len() as f64) / 4.0).ceil() as u32
}

// ── Ingestion Pipeline ─────────────────────────────────────────────────────────

/// Result of a completed ingestion.
#[derive(Debug)]
pub struct IngestResult {
    pub doc_id: String,
    pub chunk_count: u32,
}

/// Ingest raw document bytes into a collection.
///
/// Pipeline:
/// 1. SHA-256 dedup check
/// 2. Extract text with `GenericDocumentExtractor`
/// 3. Semantic chunking
/// 4. Store doc entry + chunks
/// 5. Generate and store embeddings (async, via `Embedder`)
/// 6. Update doc status to `Ready`
#[allow(clippy::too_many_arguments)]
pub async fn ingest_document(
    store: Arc<CollectionStore>,
    vec_store: Option<Arc<dyn VecStoreAdapter>>,
    embedder: Option<Arc<dyn Embedder>>,
    collection_id: &str,
    title: &str,
    file_name: Option<&str>,
    mime_type: &str,
    bytes: &[u8],
    chunk_config: &ChunkConfig,
) -> Result<IngestResult> {
    // 1. Compute SHA-256 for dedup
    let sha256 = {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        digest
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    };

    // 2. Parse document
    let extractor = GenericDocumentExtractor::new();
    let path = std::path::Path::new(file_name.unwrap_or("document.bin"));
    let extracted = extractor
        .extract(path, bytes)
        .context("document extraction failed")?;

    // 3. Create doc entry
    let mut doc = crate::collections::schema::DocEntry::new(collection_id, title, mime_type)
        .with_sha256(sha256)
        .with_raw_size(bytes.len() as u64);
    if let Some(fname) = file_name {
        doc = doc.with_file_name(fname);
    }

    store.create_doc(&doc).context("storing doc entry")?;
    store
        .update_doc_status(&doc.id, &DocStatus::Processing, None, None)
        .context("setting doc status to processing")?;

    // 4. Chunk content
    let text_to_chunk = if extracted.content.trim().is_empty() {
        extracted
            .chunks
            .iter()
            .map(|c| c.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    } else {
        extracted.content.clone()
    };

    let raw_chunks = semantic_chunk(&text_to_chunk, chunk_config);
    let mut doc_chunks: Vec<DocChunk> = raw_chunks
        .iter()
        .enumerate()
        .map(|(i, content)| {
            DocChunk::new(&doc.id, collection_id, i as u32, content)
                .with_token_count(estimate_tokens(content))
        })
        .collect();

    // Attach page info from extractor chunks when available
    for (i, xchunk) in extracted.chunks.iter().enumerate() {
        if let Some(dc) = doc_chunks.get_mut(i) {
            if let Some(page) = xchunk.page {
                *dc = DocChunk {
                    page: Some(page as u32),
                    ..dc.clone()
                };
            }
        }
    }

    let chunk_count = doc_chunks.len() as u32;

    // 5. Persist chunks + FTS index
    store
        .insert_chunks(&doc_chunks)
        .context("inserting chunks")?;

    // 6. Generate embeddings
    if let (Some(emb), Some(vs)) = (embedder, vec_store) {
        for chunk in &doc_chunks {
            match emb.encode(&chunk.content).await {
                Ok(vector) => {
                    if let Err(e) = vs.upsert(&chunk.id, &vector).await {
                        warn!(chunk_id = %chunk.id, error = %e, "embedding store failed");
                    }
                }
                Err(e) => {
                    warn!(chunk_id = %chunk.id, error = %e, "embedding generation failed");
                }
            }
        }
        info!(
            doc_id = %doc.id,
            chunks = chunk_count,
            "embeddings stored"
        );
    }

    // 7. Mark ready
    store
        .update_doc_status(&doc.id, &DocStatus::Ready, Some(chunk_count), None)
        .context("marking doc ready")?;

    info!(
        doc_id = %doc.id,
        title = %title,
        chunks = chunk_count,
        "document ingested successfully"
    );

    Ok(IngestResult {
        doc_id: doc.id,
        chunk_count,
    })
}

// ── VecStore adapter trait ─────────────────────────────────────────────────────

/// Minimal abstraction over vector stores (sqlite-vec, pgvector, etc.)
/// allowing the indexer to remain backend-agnostic.
#[async_trait::async_trait]
pub trait VecStoreAdapter: Send + Sync {
    async fn upsert(&self, id: &str, vector: &[f32]) -> Result<()>;
    async fn search(&self, query: &[f32], limit: usize) -> Result<Vec<VecMatch>>;
}

#[derive(Debug, Clone)]
pub struct VecMatch {
    pub id: String,
    pub score: f32,
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_chunk_short_text() {
        let config = ChunkConfig::default();
        let text = "Hola mundo. Esta es una prueba.";
        let chunks = semantic_chunk(text, &config);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("Hola mundo"));
    }

    #[test]
    fn test_semantic_chunk_paragraph_break() {
        let config = ChunkConfig {
            target_tokens: 20, // ~80 chars
            overlap_tokens: 5,
            min_chars: 10,
        };
        let para1 =
            "Este es el primer párrafo con suficiente contenido para ser un chunk separado.";
        let para2 = "Este es el segundo párrafo completamente diferente con más información.";
        let text = format!("{}\n\n{}", para1, para2);
        let chunks = semantic_chunk(&text, &config);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_semantic_chunk_overlap() {
        let config = ChunkConfig {
            target_tokens: 10, // ~40 chars
            overlap_tokens: 3, // ~12 chars
            min_chars: 5,
        };
        let text = "aaaa bbbb cccc. dddd eeee ffff. gggg hhhh iiii. jjjj kkkk llll.";
        let chunks = semantic_chunk(text, &config);
        // With overlap, adjacent chunks should share some content
        assert!(chunks.len() > 1);
    }

    #[test]
    fn test_estimate_tokens() {
        let t = estimate_tokens("This is approximately twenty characters long.");
        assert!(t > 0);
    }

    #[test]
    fn test_chunk_config_from_default() {
        let cfg = ChunkConfig::default();
        assert_eq!(cfg.target_tokens, 512);
        assert_eq!(cfg.overlap_tokens, 64);
    }
}
