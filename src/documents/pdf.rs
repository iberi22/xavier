//! Pure-Rust PDF document text extractor.
//!
//! Parses PDF document streams, handles FlateDecode/Zlib decompression with flate2,
//! extracts BT...ET text blocks, TJ/Tj operators, and detects scanned/image-only pages.

use crate::documents::ExtractedChunk;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Result of PDF extraction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedPdfDoc {
    pub total_pages: usize,
    pub is_scanned: bool,
    pub content: String,
    pub chunks: Vec<ExtractedChunk>,
    pub metadata: serde_json::Value,
}

/// Decodes escaped characters in PDF literal strings (e.g. \(, \), \\, \n, \r, \t).
fn decode_pdf_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                chars.next();
                match next {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\x08'),
                    'f' => out.push('\x0c'),
                    '(' => out.push('('),
                    ')' => out.push(')'),
                    '\\' => out.push('\\'),
                    other => out.push(other),
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Decodes hexadecimal strings `<48656C6C6F>` to UTF-8.
fn decode_hex_string(hex_str: &str) -> String {
    let clean: String = hex_str.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.is_empty() {
        return String::new();
    }
    if let Ok(bytes) = crate::crypto::hex_decode(&clean) {
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        clean
    }
}

/// Extracts text from PDF stream content (decodes BT ... ET text blocks).
fn extract_text_from_pdf_stream(stream_str: &str) -> String {
    let mut extracted = String::new();
    let mut in_text_block = false;

    // Iterate over tokens
    let mut chars = stream_str.chars().peekable();

    while let Some(c) = chars.next() {
        if !in_text_block {
            // Check for 'BT'
            if c == 'B' && chars.peek() == Some(&'T') {
                chars.next();
                in_text_block = true;
            }
            continue;
        }

        // Inside text block, check for 'ET'
        if c == 'E' && chars.peek() == Some(&'T') {
            chars.next();
            in_text_block = false;
            extracted.push('\n');
            continue;
        }

        // PDF string literal: (...)
        if c == '(' {
            let mut literal = String::new();
            let mut depth = 1;

            while let Some(ch) = chars.next() {
                if ch == '\\' {
                    literal.push(ch);
                    if let Some(escaped) = chars.next() {
                        literal.push(escaped);
                    }
                } else if ch == '(' {
                    depth += 1;
                    literal.push(ch);
                } else if ch == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    literal.push(ch);
                } else {
                    literal.push(ch);
                }
            }

            let decoded = decode_pdf_string(&literal);
            extracted.push_str(&decoded);
            extracted.push(' ');
        }
        // Hexadecimal string: <...>
        else if c == '<' && chars.peek() != Some(&'<') {
            let mut hex_content = String::new();
            for ch in chars.by_ref() {
                if ch == '>' {
                    break;
                }
                hex_content.push(ch);
            }
            let decoded = decode_hex_string(&hex_content);
            extracted.push_str(&decoded);
            extracted.push(' ');
        }
    }

    extracted
}

/// Decompresses flate streams or returns raw slice.
fn decompress_stream(data: &[u8]) -> Vec<u8> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    if decoder.read_to_end(&mut decompressed).is_ok() && !decompressed.is_empty() {
        decompressed
    } else {
        data.to_vec()
    }
}

/// Extracts text and structure from raw PDF bytes.
pub fn extract_pdf_document(path: &Path, bytes: &[u8]) -> Result<ExtractedPdfDoc, String> {
    if !bytes.starts_with(b"%PDF-") {
        return Err("Not a valid PDF file (missing %PDF- header)".to_string());
    }

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256_hex = crate::crypto::hex_encode(hasher.finalize());

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document.pdf");

    // Search for stream ... endstream blocks in the PDF
    let mut streams = Vec::new();
    let mut cursor = 0;

    let stream_tag = b"stream";
    let endstream_tag = b"endstream";

    while let Some(start_idx) = bytes[cursor..]
        .windows(stream_tag.len())
        .position(|w| w == stream_tag)
    {
        let stream_start = cursor + start_idx + stream_tag.len();

        // Skip newline after 'stream' (\r\n or \n)
        let actual_start = if stream_start < bytes.len()
            && bytes[stream_start] == b'\r'
            && stream_start + 1 < bytes.len()
            && bytes[stream_start + 1] == b'\n'
        {
            stream_start + 2
        } else if stream_start < bytes.len()
            && (bytes[stream_start] == b'\n' || bytes[stream_start] == b'\r')
        {
            stream_start + 1
        } else {
            stream_start
        };

        if let Some(end_idx) = bytes[actual_start..]
            .windows(endstream_tag.len())
            .position(|w| w == endstream_tag)
        {
            let actual_end = actual_start + end_idx;
            // Trim trailing \r\n if present before endstream
            let mut stream_data = &bytes[actual_start..actual_end];
            if stream_data.ends_with(b"\r\n") {
                stream_data = &stream_data[..stream_data.len() - 2];
            } else if stream_data.ends_with(b"\n") || stream_data.ends_with(b"\r") {
                stream_data = &stream_data[..stream_data.len() - 1];
            }

            streams.push(stream_data);
            cursor = actual_end + endstream_tag.len();
        } else {
            break;
        }
    }

    let mut full_text = String::new();
    let mut chunks = Vec::new();
    let mut page_counter = 1;

    for stream_bytes in streams {
        let decompressed = decompress_stream(stream_bytes);
        let stream_str = String::from_utf8_lossy(&decompressed);

        let page_text = extract_text_from_pdf_stream(&stream_str);
        let trimmed = page_text.trim();

        if !trimmed.is_empty() {
            let page_header = format!("--- [PÁGINA {}] ---\n", page_counter);
            full_text.push_str(&page_header);
            full_text.push_str(trimmed);
            full_text.push_str("\n\n");

            chunks.push(ExtractedChunk {
                index: chunks.len(),
                content: format!("{}{}", page_header, trimmed),
                page: Some(page_counter),
                start_line: 0,
                end_line: trimmed.lines().count(),
                metadata: serde_json::json!({
                    "type": "pdf_page",
                    "page": page_counter,
                    "file": filename,
                }),
            });

            page_counter += 1;
        }
    }

    let total_pages = if chunks.is_empty() { 1 } else { chunks.len() };
    let is_scanned = full_text.trim().len() < 20;

    if is_scanned {
        full_text = format!(
            "[DOCUMENTO PDF ESCANEADO / RASTER: {}]\nPáginas detectadas: {}\nAtención: No se detectaron capas de texto vectorial embebidas (posible documento escaneado/imagen).\nRecomendación: Procesar mediante OCR / Vision Model para transcripción textual completa.",
            filename, total_pages
        );

        if chunks.is_empty() {
            chunks.push(ExtractedChunk {
                index: 0,
                content: full_text.clone(),
                page: Some(1),
                start_line: 0,
                end_line: full_text.lines().count(),
                metadata: serde_json::json!({
                    "type": "pdf_scanned",
                    "is_scanned": true,
                    "file": filename,
                }),
            });
        }
    }

    let metadata = serde_json::json!({
        "type": "pdf",
        "total_pages": total_pages,
        "is_scanned": is_scanned,
        "sha256": sha256_hex,
        "file": filename,
    });

    Ok(ExtractedPdfDoc {
        total_pages,
        is_scanned,
        content: full_text,
        chunks,
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pdf_stream_text() {
        let stream =
            "BT /F1 12 Tf 72 712 Td (Contrato de Arrendamiento) Tj ET BT (Clausula Primera) Tj ET";
        let text = extract_text_from_pdf_stream(stream);
        assert!(text.contains("Contrato de Arrendamiento"));
        assert!(text.contains("Clausula Primera"));
    }

    #[test]
    fn test_extract_synthetic_pdf() {
        let pdf_data = b"%PDF-1.4\n1 0 obj\n<< /Length 50 >>\nstream\nBT (Poder Especial Judicial) Tj ET\nendstream\nendobj\n";
        let doc = extract_pdf_document(Path::new("poder.pdf"), pdf_data).expect("extract pdf");
        assert!(!doc.is_scanned);
        assert!(doc.content.contains("Poder Especial Judicial"));
        assert_eq!(doc.chunks.len(), 1);
    }

    #[test]
    fn test_scanned_pdf_detection() {
        let empty_pdf = b"%PDF-1.4\n1 0 obj\n<< >>\nstream\n\nendstream\nendobj\n";
        let doc = extract_pdf_document(Path::new("escaneado.pdf"), empty_pdf).expect("extract pdf");
        assert!(doc.is_scanned);
        assert!(doc.content.contains("DOCUMENTO PDF ESCANEADO"));
    }
}
