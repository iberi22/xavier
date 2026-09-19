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

/// Decodes escaped characters in PDF literal strings (e.g. \(, \), \\, \n, \r, \t, and octal escapes like \301).
fn decode_pdf_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if ('0'..='7').contains(&next) {
                    // Octal escape sequence \ddd (1 to 3 octal digits)
                    let mut octal_val = 0u32;
                    let mut count = 0;
                    while count < 3 {
                        if let Some(&d) = chars.peek() {
                            if ('0'..='7').contains(&d) {
                                chars.next();
                                octal_val = octal_val * 8 + (d as u32 - '0' as u32);
                                count += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                    let byte = (octal_val & 0xFF) as u8;
                    // Decode byte using Windows-1252 / ISO-8859-1
                    let ch = match byte {
                        0x80 => '€',
                        0x82 => '‚',
                        0x83 => 'ƒ',
                        0x84 => '„',
                        0x85 => '…',
                        0x86 => '†',
                        0x87 => '‡',
                        0x88 => 'ˆ',
                        0x89 => '‰',
                        0x8A => 'Š',
                        0x8B => '‹',
                        0x8C => 'Œ',
                        0x8E => 'Ž',
                        0x91 => '‘',
                        0x92 => '’',
                        0x93 => '“',
                        0x94 => '”',
                        0x95 => '•',
                        0x96 => '–',
                        0x97 => '—',
                        0x98 => '˜',
                        0x99 => '™',
                        0x9A => 'š',
                        0x9B => '›',
                        0x9C => 'œ',
                        0x9E => 'ž',
                        0x9F => 'Ÿ',
                        b => b as char,
                    };
                    out.push(ch);
                } else {
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
                    literal.push('\\');
                    if let Some(escaped) = chars.next() {
                        literal.push(escaped);
                        // If it's an octal sequence (\ddd), capture remaining up to 2 octal digits
                        if ('0'..='7').contains(&escaped) {
                            for _ in 0..2 {
                                if let Some(&d) = chars.peek() {
                                    if ('0'..='7').contains(&d) {
                                        literal.push(chars.next().unwrap());
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
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

#[derive(Debug, Clone)]
struct RawPdfObject {
    id: u32,
    gen: u32,
    header: String,
    stream: Option<Vec<u8>>,
}

fn parse_pdf_objects(bytes: &[u8]) -> std::collections::HashMap<u32, RawPdfObject> {
    let mut objects = std::collections::HashMap::new();
    let mut cursor = 0;
    let obj_tag = b"obj";
    let endobj_tag = b"endobj";

    while let Some(pos) = bytes[cursor..]
        .windows(obj_tag.len())
        .position(|w| w == obj_tag)
    {
        let obj_pos = cursor + pos;
        let after_obj = obj_pos + 3;
        if after_obj < bytes.len() && !bytes[after_obj].is_ascii_whitespace() {
            cursor = after_obj;
            continue;
        }

        // Look backward for "N M"
        let pre_start = obj_pos.saturating_sub(40);
        let pre_str = String::from_utf8_lossy(&bytes[pre_start..obj_pos]);
        let tokens: Vec<&str> = pre_str.split_whitespace().collect();
        if tokens.len() >= 2 {
            if let (Ok(id), Ok(gen)) = (
                tokens[tokens.len() - 2].parse::<u32>(),
                tokens[tokens.len() - 1].parse::<u32>(),
            ) {
                if let Some(end_pos) = bytes[after_obj..]
                    .windows(endobj_tag.len())
                    .position(|w| w == endobj_tag)
                {
                    let obj_end = after_obj + end_pos;
                    let body = &bytes[after_obj..obj_end];

                    let stream_tag = b"stream";
                    let endstream_tag = b"endstream";
                    let (header, stream) = if let Some(s_pos) =
                        body.windows(stream_tag.len()).position(|w| w == stream_tag)
                    {
                        let head_bytes = &body[..s_pos];
                        let s_start = s_pos + stream_tag.len();
                        let s_actual_start = if s_start < body.len()
                            && body[s_start] == b'\r'
                            && s_start + 1 < body.len()
                            && body[s_start + 1] == b'\n'
                        {
                            s_start + 2
                        } else if s_start < body.len()
                            && (body[s_start] == b'\n' || body[s_start] == b'\r')
                        {
                            s_start + 1
                        } else {
                            s_start
                        };

                        if let Some(e_pos) = body[s_actual_start..]
                            .windows(endstream_tag.len())
                            .position(|w| w == endstream_tag)
                        {
                            let s_end = s_actual_start + e_pos;
                            let mut s_data = &body[s_actual_start..s_end];
                            if s_data.ends_with(b"\r\n") {
                                s_data = &s_data[..s_data.len() - 2];
                            } else if s_data.ends_with(b"\n") || s_data.ends_with(b"\r") {
                                s_data = &s_data[..s_data.len() - 1];
                            }
                            (
                                String::from_utf8_lossy(head_bytes).to_string(),
                                Some(s_data.to_vec()),
                            )
                        } else {
                            (String::from_utf8_lossy(body).to_string(), None)
                        }
                    } else {
                        (String::from_utf8_lossy(body).to_string(), None)
                    };

                    objects.insert(
                        id,
                        RawPdfObject {
                            id,
                            gen,
                            header,
                            stream,
                        },
                    );
                    cursor = obj_end + endobj_tag.len();
                    continue;
                }
            }
        }
        cursor = obj_pos + obj_tag.len();
    }

    objects
}

fn collect_page_ids(
    page_node_id: u32,
    objects: &std::collections::HashMap<u32, RawPdfObject>,
    out: &mut Vec<u32>,
    visited: &mut std::collections::HashSet<u32>,
) {
    if !visited.insert(page_node_id) {
        return;
    }
    if let Some(node) = objects.get(&page_node_id) {
        let is_page = (node.header.contains("/Type /Page ")
            || node.header.contains("/Type/Page ")
            || node.header.contains("/Type /Page\n")
            || node.header.contains("/Type /Page\r")
            || node.header.contains("/Type /Page>>")
            || node.header.contains("/Type/Page>>"))
            && !node.header.contains("/Pages");

        if is_page {
            out.push(page_node_id);
            return;
        }

        // It is a /Pages node: extract /Kids [ ... ]
        if let Some(kids_idx) = node.header.find("/Kids") {
            if let Some(b_open) = node.header[kids_idx..].find('[') {
                let rest = &node.header[kids_idx + b_open + 1..];
                if let Some(b_close) = rest.find(']') {
                    let kids_str = &rest[..b_close];
                    let tokens: Vec<&str> = kids_str.split_whitespace().collect();
                    for (i, &t) in tokens.iter().enumerate() {
                        if t == "R" && i >= 2 {
                            if let Ok(kid_id) = tokens[i - 2].parse::<u32>() {
                                collect_page_ids(kid_id, objects, out, visited);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Extracts text and structure from raw PDF bytes.
pub fn extract_pdf_document(path: &Path, bytes: &[u8]) -> Result<ExtractedPdfDoc, String> {
    if !bytes.starts_with(b"%PDF-") {
        return Err("Not a valid PDF file (missing %PDF- header)".to_string());
    }

    // Require minimal valid PDF structure: must contain "obj" or "xref"
    let has_obj = bytes.windows(3).any(|w| w == b"obj");
    let has_xref = bytes.windows(4).any(|w| w == b"xref");
    let has_trailer = bytes.windows(7).any(|w| w == b"trailer");

    if !has_obj && !has_xref && !has_trailer {
        return Err("Malformed PDF: missing valid PDF structure (no objects or xref)".to_string());
    }

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256_hex = crate::crypto::hex_encode(hasher.finalize());

    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("document.pdf");

    let objects = parse_pdf_objects(bytes);
    if objects.is_empty() {
        return Err("Malformed PDF: no valid objects found".to_string());
    }

    // Resolve Page Tree starting from Catalog
    let catalog = objects
        .values()
        .find(|obj| obj.header.contains("/Type /Catalog") || obj.header.contains("/Type/Catalog"));

    let mut ordered_page_ids = Vec::new();
    if let Some(cat) = catalog {
        // Find /Pages <id> 0 R
        let tokens: Vec<&str> = cat.header.split_whitespace().collect();
        let pages_root = tokens.iter().enumerate().find_map(|(i, &tok)| {
            if tok == "/Pages" && i + 2 < tokens.len() && tokens[i + 2] == "R" {
                tokens[i + 1].parse::<u32>().ok()
            } else {
                None
            }
        });

        if let Some(root_id) = pages_root {
            let mut visited = std::collections::HashSet::new();
            collect_page_ids(root_id, &objects, &mut ordered_page_ids, &mut visited);
        }
    }

    // Fallback: if no Catalog/Pages tree was resolved, find all /Page objects
    if ordered_page_ids.is_empty() {
        let mut all_pages: Vec<u32> = objects
            .iter()
            .filter(|(_, obj)| {
                (obj.header.contains("/Type /Page ") || obj.header.contains("/Type/Page "))
                    && !obj.header.contains("/Pages")
            })
            .map(|(&id, _)| id)
            .collect();
        all_pages.sort();
        ordered_page_ids = all_pages;
    }

    // Secondary fallback: if still no /Page objects found, take objects with streams as pages
    if ordered_page_ids.is_empty() {
        let mut stream_objs: Vec<u32> = objects
            .iter()
            .filter(|(_, obj)| obj.stream.is_some())
            .map(|(&id, _)| id)
            .collect();
        stream_objs.sort();
        ordered_page_ids = stream_objs;
    }

    let mut full_text = String::new();
    let mut chunks = Vec::new();

    if !ordered_page_ids.is_empty() {
        let total_pages = ordered_page_ids.len();

        for (page_idx, &page_id) in ordered_page_ids.iter().enumerate() {
            let page_num = page_idx + 1;
            let page_obj = match objects.get(&page_id) {
                Some(p) => p,
                None => continue,
            };

            // Find /Contents references (either "N 0 R" or "[ N 0 R M 0 R ]")
            let mut content_stream_ids = Vec::new();
            if let Some(contents_idx) = page_obj.header.find("/Contents") {
                let rest = &page_obj.header[contents_idx + 9..];
                if let Some(bracket_start) = rest.find('[') {
                    if let Some(bracket_end) = rest.find(']') {
                        let inner = &rest[bracket_start + 1..bracket_end];
                        let tokens: Vec<&str> = inner.split_whitespace().collect();
                        for (i, &t) in tokens.iter().enumerate() {
                            if t == "R" && i >= 2 {
                                if let Ok(cid) = tokens[i - 2].parse::<u32>() {
                                    content_stream_ids.push(cid);
                                }
                            }
                        }
                    }
                } else {
                    let tokens: Vec<&str> = rest.split_whitespace().collect();
                    for (i, &t) in tokens.iter().enumerate() {
                        if t == "R" && i >= 2 {
                            if let Ok(cid) = tokens[i - 2].parse::<u32>() {
                                content_stream_ids.push(cid);
                                break;
                            }
                        }
                    }
                }
            }

            if content_stream_ids.is_empty() && page_obj.stream.is_some() {
                content_stream_ids.push(page_id);
            }

            let mut page_text = String::new();

            for stream_id in content_stream_ids {
                if let Some(stream_obj) = objects.get(&stream_id) {
                    if let Some(ref raw_stream) = stream_obj.stream {
                        let decompressed = decompress_stream(raw_stream);
                        let stream_str = String::from_utf8_lossy(&decompressed);
                        let extracted = extract_text_from_pdf_stream(&stream_str);
                        let trimmed = extracted.trim();
                        if !trimmed.is_empty() {
                            if !page_text.is_empty() {
                                page_text.push(' ');
                            }
                            page_text.push_str(trimmed);
                        }
                    }
                }
            }

            // If page text is empty, check for XObject raster images (Scanned PDF OCR)
            if page_text.trim().is_empty() {
                // Search for XObject references
                for obj in objects.values() {
                    if obj.header.contains("/Subtype /Image")
                        || obj.header.contains("/Subtype/Image")
                    {
                        if let Some(ref raw_img) = obj.stream {
                            let decompressed = decompress_stream(raw_img);

                            // Extract Width and Height from image header
                            let width = obj
                                .header
                                .split_whitespace()
                                .enumerate()
                                .find_map(|(i, t)| {
                                    if t == "/Width" {
                                        obj.header
                                            .split_whitespace()
                                            .nth(i + 1)
                                            .and_then(|w| w.parse::<u32>().ok())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);

                            let height = obj
                                .header
                                .split_whitespace()
                                .enumerate()
                                .find_map(|(i, t)| {
                                    if t == "/Height" {
                                        obj.header
                                            .split_whitespace()
                                            .nth(i + 1)
                                            .and_then(|h| h.parse::<u32>().ok())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);

                            if width > 0
                                && height > 0
                                && decompressed.len() as u32 >= width * height
                            {
                                let pixels = &decompressed[..(width * height) as usize];
                                if let Some(gray) =
                                    image::GrayImage::from_raw(width, height, pixels.to_vec())
                                {
                                    if let Some(ocr) = crate::documents::image_extractor::ImageExtractor::decode_gray_image(&gray) {
                                        if !ocr.is_empty() {
                                            page_text.push_str(&ocr);
                                        }
                                    }
                                }
                            } else if let Ok(img) = image::load_from_memory(&decompressed) {
                                let gray = img.to_luma8();
                                if let Some(ocr) = crate::documents::image_extractor::ImageExtractor::decode_gray_image(&gray) {
                                    if !ocr.is_empty() {
                                        page_text.push_str(&ocr);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            let trimmed = page_text.trim();
            if !trimmed.is_empty() {
                let page_header = format!("--- [PÁGINA {}] ---\n", page_num);
                full_text.push_str(&page_header);
                full_text.push_str(trimmed);
                full_text.push_str("\n\n");

                chunks.push(ExtractedChunk {
                    index: chunks.len(),
                    content: format!("{}{}", page_header, trimmed),
                    page: Some(page_num),
                    start_line: 0,
                    end_line: trimmed.lines().count(),
                    metadata: serde_json::json!({
                        "type": "pdf_page",
                        "page": page_num,
                        "file": filename,
                    }),
                });
            }
        }

        let mut is_scanned = full_text.trim().len() < 20;

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
            is_scanned = true;
        }

        let metadata = serde_json::json!({
            "type": "pdf",
            "total_pages": total_pages,
            "is_scanned": is_scanned,
            "sha256": sha256_hex,
            "file": filename,
        });

        return Ok(ExtractedPdfDoc {
            total_pages,
            is_scanned,
            content: full_text,
            chunks,
            metadata,
        });
    }

    // Fallback if no page objects found at all
    Err("Malformed PDF: no readable pages found".to_string())
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
