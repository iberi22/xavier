//! Image document extractor with perceptual hashing and visual metadata.
//!
//! Provides 64-bit dHash (difference hash) calculation, structural dimension analysis,
//! and metadata generation for vectorization and semantic search in Xavier.

use crate::documents::ExtractedChunk;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Extracted image document representation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractedImageDoc {
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: String,
    pub format: String,
    pub dhash: String,
    pub sha256: String,
    pub summary_text: String,
    pub chunk: ExtractedChunk,
}

/// Computes a 64-bit difference hash (dHash) from raw image bytes.
/// Resizes to 9x8 grayscale and compares adjacent horizontal gradients.
pub fn compute_dhash(bytes: &[u8]) -> Result<(u64, u32, u32, String), String> {
    let img =
        ::image::load_from_memory(bytes).map_err(|e| format!("Failed to decode image: {}", e))?;

    let width = img.width();
    let height = img.height();
    let format = format!("{:?}", img.color());

    let gray = img.to_luma8();
    let resized = ::image::imageops::resize(&gray, 9, 8, ::image::imageops::FilterType::Nearest);

    let mut hash: u64 = 0;
    let mut bit = 0;

    for y in 0..8 {
        for x in 0..8 {
            let left = resized.get_pixel(x, y)[0];
            let right = resized.get_pixel(x + 1, y)[0];
            if left > right {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }

    Ok((hash, width, height, format))
}

/// Computes the Hamming distance between two 64-bit perceptual hashes.
/// - 0: Identical images
/// - <= 5: Near-duplicate (recompressed, resized, or minor watermark)
/// - <= 10: Structurally similar visual composition
pub fn hamming_distance(hash_a: u64, hash_b: u64) -> u32 {
    (hash_a ^ hash_b).count_ones()
}

/// Calculates human-readable aspect ratio.
fn calculate_aspect_ratio(width: u32, height: u32) -> String {
    if height == 0 {
        return "unknown".to_string();
    }
    let ratio = width as f32 / height as f32;
    if (ratio - 1.777).abs() < 0.05 {
        "16:9".to_string()
    } else if (ratio - 1.333).abs() < 0.05 {
        "4:3".to_string()
    } else if (ratio - 1.0).abs() < 0.05 {
        "1:1".to_string()
    } else if (ratio - 0.5625).abs() < 0.05 {
        "9:16 (vertical)".to_string()
    } else {
        format!("{:.2}:1", ratio)
    }
}

/// Extracts metadata and perceptual hash from image bytes.
pub fn extract_image_document(path: &Path, bytes: &[u8]) -> Result<ExtractedImageDoc, String> {
    let (hash_val, width, height, color_format) = compute_dhash(bytes)?;
    let dhash_hex = format!("{:016x}", hash_val);

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256_hex = crate::crypto::hex_encode(hasher.finalize());

    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("image");
    let aspect_ratio = calculate_aspect_ratio(width, height);

    let summary_text = format!(
        "[IMAGE DOCUMENT: {}]\nResolution: {}x{} | Ratio: {}\nColor Space: {}\nPerceptual dHash: {}\nSHA-256: {}\nByte Size: {} bytes\nStatus: Visual asset indexed for cognitive retrieval.",
        filename, width, height, aspect_ratio, color_format, dhash_hex, sha256_hex, bytes.len()
    );

    let chunk = ExtractedChunk {
        index: 0,
        content: summary_text.clone(),
        page: None,
        start_line: 0,
        end_line: summary_text.lines().count(),
        metadata: serde_json::json!({
            "type": "image",
            "width": width,
            "height": height,
            "aspect_ratio": aspect_ratio,
            "dhash": dhash_hex,
            "sha256": sha256_hex,
            "file": filename,
        }),
    };

    Ok(ExtractedImageDoc {
        width,
        height,
        aspect_ratio,
        format: color_format,
        dhash: dhash_hex,
        sha256: sha256_hex,
        summary_text,
        chunk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hamming_distance() {
        assert_eq!(hamming_distance(0x0, 0x0), 0);
        assert_eq!(hamming_distance(0b0001, 0b0000), 1);
        assert_eq!(hamming_distance(0b1111, 0b0000), 4);
    }

    #[test]
    fn test_synthetic_image_dhash() {
        // Create an in-memory 16x16 synthetic image
        let img = ::image::RgbImage::from_fn(16, 16, |x, _y| {
            if x < 8 {
                ::image::Rgb([255, 255, 255])
            } else {
                ::image::Rgb([0, 0, 0])
            }
        });

        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, ::image::ImageFormat::Png)
            .expect("write png");
        let png_bytes = buf.into_inner();

        let doc = extract_image_document(Path::new("test.png"), &png_bytes).expect("extract");
        assert_eq!(doc.width, 16);
        assert_eq!(doc.height, 16);
        assert_eq!(doc.aspect_ratio, "1:1");
        assert!(!doc.dhash.is_empty());
        assert!(doc.summary_text.contains("[IMAGE DOCUMENT: test.png]"));
    }
}
