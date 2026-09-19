//! Generic Multimodal Document Ingestion Engine for Xavier.
//!
//! Provides unified parsing, chunking, and metadata extraction for:
//! - Plain text / Markdown / Source Code
//! - Adobe PDF (text streams, FlateDecode decompression, page segmentation, scan detection)
//! - Images (PNG, JPEG, WEBP, GIF, BMP with 64-bit dHash perceptual hashing and visual metadata)

pub mod audio_transcriber;
pub mod detector;
pub mod image_extractor;
pub mod img;
pub mod legal_chunker;
pub mod pdf;
pub mod video_segmenter;

#[cfg(test)]
mod tests_audio_transcriber;
#[cfg(test)]
mod tests_image_extractor;
#[cfg(test)]
mod tests_video_segmenter;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use detector::{detect_document_format, DetectedFormat};
pub use img::{compute_dhash, extract_image_document, hamming_distance, ExtractedImageDoc};
pub use pdf::{extract_pdf_document, ExtractedPdfDoc};

/// Document kind classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Markdown,
    Text,
    Pdf,
    Image,
    Unknown,
}

/// Extracted text/data chunk ready for embedding generation and search indexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedChunk {
    pub index: usize,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    pub start_line: usize,
    pub end_line: usize,
    pub metadata: serde_json::Value,
}

/// Unified document intermediate representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedDocument {
    pub path: String,
    pub kind: DocumentKind,
    pub mime_type: String,
    pub raw_size: usize,
    pub content: String,
    pub chunks: Vec<ExtractedChunk>,
    pub metadata: serde_json::Value,
}

/// Generic Multimodal Document Extractor.
#[derive(Debug, Clone, Default)]
pub struct GenericDocumentExtractor;

impl GenericDocumentExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Extracts document content, chunks, and metadata from raw bytes.
    pub fn extract(&self, path: &Path, bytes: &[u8]) -> Result<ExtractedDocument> {
        let format = detect_document_format(bytes, Some(path));
        let path_str = path.to_string_lossy().to_string();
        let raw_size = bytes.len();

        match format {
            DetectedFormat::Pdf => {
                let doc = extract_pdf_document(path, bytes)
                    .map_err(|e| anyhow::anyhow!("PDF extraction failed: {}", e))?;
                Ok(ExtractedDocument {
                    path: path_str,
                    kind: DocumentKind::Pdf,
                    mime_type: format.mime_type().to_string(),
                    raw_size,
                    content: doc.content,
                    chunks: doc.chunks,
                    metadata: doc.metadata,
                })
            }
            DetectedFormat::Png
            | DetectedFormat::Jpeg
            | DetectedFormat::Webp
            | DetectedFormat::Gif => {
                let doc = extract_image_document(path, bytes)
                    .map_err(|e| anyhow::anyhow!("Image extraction failed: {}", e))?;
                Ok(ExtractedDocument {
                    path: path_str,
                    kind: DocumentKind::Image,
                    mime_type: format.mime_type().to_string(),
                    raw_size,
                    content: doc.summary_text,
                    chunks: vec![doc.chunk],
                    metadata: serde_json::json!({
                        "type": "image",
                        "width": doc.width,
                        "height": doc.height,
                        "aspect_ratio": doc.aspect_ratio,
                        "dhash": doc.dhash,
                        "sha256": doc.sha256,
                    }),
                })
            }
            DetectedFormat::Markdown | DetectedFormat::Text => {
                let text = std::str::from_utf8(bytes)
                    .with_context(|| format!("Invalid UTF-8 in text document: {:?}", path))?;
                let kind = if format == DetectedFormat::Markdown {
                    DocumentKind::Markdown
                } else {
                    DocumentKind::Text
                };

                let chunks = if self.is_legal_document(text) {
                    self.generate_legal_chunks(&path_str, text)
                } else {
                    self.generate_text_chunks(text)
                };

                Ok(ExtractedDocument {
                    path: path_str,
                    kind,
                    mime_type: format.mime_type().to_string(),
                    raw_size,
                    content: text.to_string(),
                    chunks,
                    metadata: serde_json::json!({
                        "type": "text",
                        "lines": text.lines().count(),
                    }),
                })
            }
            DetectedFormat::BinaryUnknown => {
                anyhow::bail!(
                    "Unsupported or unknown binary document format for: {:?}",
                    path
                )
            }
        }
    }

    /// Generates chunks for text content.
    fn generate_text_chunks(&self, content: &str) -> Vec<ExtractedChunk> {
        let mut chunks = Vec::new();
        let lines: Vec<&str> = content.lines().collect();

        let mut current_chunk = String::new();
        let mut start_line = 0;
        let mut line_count = 0;

        for (i, line) in lines.iter().enumerate() {
            current_chunk.push_str(line);
            current_chunk.push('\n');
            line_count += 1;

            if line_count >= 20 || (line.is_empty() && current_chunk.len() > 200) {
                if !current_chunk.trim().is_empty() {
                    chunks.push(ExtractedChunk {
                        index: chunks.len(),
                        content: current_chunk.clone(),
                        page: None,
                        start_line,
                        end_line: i,
                        metadata: serde_json::json!({ "type": "text_chunk" }),
                    });
                }
                current_chunk.clear();
                start_line = i + 1;
                line_count = 0;
            }
        }

        if !current_chunk.trim().is_empty() {
            chunks.push(ExtractedChunk {
                index: chunks.len(),
                content: current_chunk,
                page: None,
                start_line,
                end_line: lines.len(),
                metadata: serde_json::json!({ "type": "text_chunk" }),
            });
        }

        chunks
    }

    /// Checks if text content contains legal contract structural markers.
    fn is_legal_document(&self, content: &str) -> bool {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && legal_chunker::detect_header(trimmed).is_some() {
                return true;
            }
        }
        false
    }

    /// Uses `LegalHierarchicalChunker` to chunk legal text and attach clause metadata.
    fn generate_legal_chunks(&self, doc_id: &str, content: &str) -> Vec<ExtractedChunk> {
        let chunker = legal_chunker::LegalHierarchicalChunker::default();
        let legal_chunks = chunker.chunk_document(doc_id, content);

        let lines: Vec<&str> = content.lines().collect();

        legal_chunks
            .into_iter()
            .enumerate()
            .map(|(i, lc)| {
                // Approximate line boundaries
                let start_line = lines
                    .iter()
                    .position(|l| l.contains(lc.text.lines().next().unwrap_or("").trim()))
                    .unwrap_or(0);
                let end_line = start_line + lc.text.lines().count();

                let mut meta_map = serde_json::Map::new();
                meta_map.insert("type".to_string(), serde_json::json!("legal_chunk"));
                meta_map.insert("full_path".to_string(), serde_json::json!(lc.full_path));
                meta_map.insert(
                    "clause_title".to_string(),
                    serde_json::json!(lc.clause_title),
                );

                if let Some(num) = lc.clause_number {
                    meta_map.insert("clause_number".to_string(), serde_json::json!(num));
                }

                for (k, v) in lc.metadata {
                    if k != "full_path" && k != "clause_title" && k != "clause_number" {
                        meta_map.insert(k, serde_json::json!(v));
                    }
                }

                ExtractedChunk {
                    index: i,
                    content: lc.text,
                    page: None,
                    start_line,
                    end_line,
                    metadata: serde_json::Value::Object(meta_map),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_pipeline_text() {
        let pipeline = GenericDocumentExtractor::new();
        let content = b"# Header\nLine 1\nLine 2";
        let doc = pipeline
            .extract(Path::new("test.md"), content)
            .expect("extract");
        assert_eq!(doc.kind, DocumentKind::Markdown);
        assert_eq!(doc.chunks.len(), 1);
    }
}
