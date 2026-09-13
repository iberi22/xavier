//! Unit tests for image RAG extractor, DCT perceptual hashing, and safe EXIF parsing.

use crate::documents::image_extractor::{ImageExifMetadata, ImageExtractor, ImageRagPayload};

#[test]
fn test_phash_computation_8x8_matrix_and_determinism() {
    // Known 8x8 bitmap array with checkerboard pattern
    let mut checkerboard = [0u8; 64];
    for y in 0..8 {
        for x in 0..8 {
            if (x + y) % 2 == 0 {
                checkerboard[y * 8 + x] = 255;
            } else {
                checkerboard[y * 8 + x] = 0;
            }
        }
    }

    let hash1 = ImageExtractor::compute_phash(&checkerboard).expect("pHash calculation failed");
    let hash2 = ImageExtractor::compute_phash(&checkerboard).expect("pHash calculation failed");

    // Determinism test
    assert_eq!(hash1, hash2, "pHash output must be deterministic");

    // Solid gray 8x8 bitmap
    let solid_gray = [128u8; 64];
    let hash_solid = ImageExtractor::compute_phash(&solid_gray).expect("pHash calculation failed");

    // Hashes for distinct patterns should be different
    assert_ne!(
        hash1, hash_solid,
        "Checkerboard and solid gray hashes should differ"
    );
}

#[test]
fn test_phash_hamming_distance_deduplication() {
    let hash1 = 0b11110000_10101010_11001100_11111111_00000000_11110000_00001111_10101010u64;
    // Exactly 2 bits flipped
    let hash2 = 0b11110000_10101010_11001100_11111111_00000000_11110000_00001111_10101001u64;

    let dist = ImageExtractor::hamming_distance(hash1, hash2);
    assert_eq!(dist, 2, "Hamming distance should reflect bit differences");

    let same_dist = ImageExtractor::hamming_distance(hash1, hash1);
    assert_eq!(
        same_dist, 0,
        "Identical hashes must have hamming distance of 0"
    );
}

#[test]
fn test_exif_graceful_degradation_corrupted_and_empty_bytes() {
    // Empty bytes buffer
    let empty_payload = ImageExtractor::extract_payload(&[], "test_empty.jpg")
        .expect("Empty byte buffer should degrade gracefully");
    assert_eq!(empty_payload.phash, 0);
    assert_eq!(empty_payload.exif.camera_model, None);
    assert_eq!(empty_payload.exif.gps_coords, None);

    // Corrupted pseudo-JPEG header
    let corrupted_jpeg = vec![
        0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x10, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00, 0x12, 0x34,
    ];
    let corrupted_payload = ImageExtractor::extract_payload(&corrupted_jpeg, "corrupted.jpg")
        .expect("Corrupted JPEG header should degrade gracefully without panic");
    assert_eq!(corrupted_payload.exif.camera_model, None);
    assert_eq!(corrupted_payload.exif.iso, None);
}

#[test]
fn test_synthetic_exif_jpeg_parsing() {
    // Construct a minimal valid JPEG APP1 EXIF structure with Camera Model "XavierCam"
    let mut jpeg = vec![0xFF, 0xD8]; // SOI

    // APP1 Header
    let mut app1_body = Vec::new();
    app1_body.extend_from_slice(b"Exif\0\0");

    // TIFF Header (Little Endian "II", 0x002A, offset 8)
    app1_body.extend_from_slice(b"II\x2A\x00\x08\x00\x00\x00");

    // IFD0: 2 entries
    app1_body.extend_from_slice(&2u16.to_le_bytes()); // entry count

    // Entry 1: Model (0x0110), Type 2 (ASCII), count 10, offset 38
    app1_body.extend_from_slice(&0x0110u16.to_le_bytes());
    app1_body.extend_from_slice(&2u16.to_le_bytes());
    app1_body.extend_from_slice(&10u32.to_le_bytes());
    app1_body.extend_from_slice(&38u32.to_le_bytes());

    // Entry 2: ImageWidth (0x0100), Type 3 (SHORT), count 1, value 1920
    app1_body.extend_from_slice(&0x0100u16.to_le_bytes());
    app1_body.extend_from_slice(&3u16.to_le_bytes());
    app1_body.extend_from_slice(&1u32.to_le_bytes());
    app1_body.extend_from_slice(&1920u32.to_le_bytes());

    // Next IFD offset (0)
    app1_body.extend_from_slice(&0u32.to_le_bytes());

    // Value data for Model at offset 38 (relative to TIFF header start)
    // Offset calculation: 8 (tiff header) + 2 (num entries) + 24 (2 entries * 12) + 4 (next offset) = 38
    app1_body.extend_from_slice(b"XavierCam\0");

    // APP1 marker and length
    jpeg.push(0xFF);
    jpeg.push(0xE1);
    let app1_len = (app1_body.len() + 2) as u16;
    jpeg.extend_from_slice(&app1_len.to_be_bytes());
    jpeg.extend_from_slice(&app1_body);

    // EOI
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    let payload = ImageExtractor::extract_payload(&jpeg, "sample.jpg")
        .expect("Payload extraction should parse synthetic EXIF JPEG");

    assert_eq!(payload.exif.camera_model.as_deref(), Some("XavierCam"));
    assert_eq!(payload.exif.width, 1920);
    assert!(payload.image_id.starts_with("img_"));
    assert!(payload.tags.contains(&"jpg".to_string()));
    assert!(payload.tags.contains(&"xaviercam".to_string()));
}

#[test]
fn test_png_dimensions_and_payload_extraction() {
    // Minimal valid 1x1 PNG file
    let png_bytes: [u8; 67] = [
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, // IHDR chunk length
        0x49, 0x48, 0x44, 0x52, // "IHDR"
        0x00, 0x00, 0x00, 0x01, // width: 1
        0x00, 0x00, 0x00, 0x01, // height: 1
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, // bit depth, color, crc
        0x00, 0x00, 0x00, 0x0A, // IDAT length
        0x49, 0x44, 0x41, 0x54, // "IDAT"
        0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D,
        0xB4, // crc
        0x00, 0x00, 0x00, 0x00, // IEND length
        0x49, 0x45, 0x4E, 0x44, // "IEND"
        0xAE, 0x42, 0x60, 0x82, // crc
    ];

    let payload = ImageExtractor::extract_payload(&png_bytes, "icon.png")
        .expect("Payload extraction should succeed for PNG image");

    assert_eq!(payload.exif.width, 1);
    assert_eq!(payload.exif.height, 1);
    assert!(payload.tags.contains(&"png".to_string()));
    assert!(payload
        .caption
        .expect("caption should exist")
        .contains("icon.png"));
}
