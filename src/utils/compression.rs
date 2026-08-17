use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::io::Cursor;

/// Serializes a struct to CBOR and compresses it using Zstd.
/// This significantly reduces storage size for large JSON/Metadata objects.
pub fn compress_payload<T: Serialize>(payload: &T) -> Result<Vec<u8>> {
    // Serialize to CBOR
    let mut cbor_data = Vec::new();
    ciborium::into_writer(payload, &mut cbor_data)
        .context("Failed to serialize payload to CBOR")?;

    // Compress with Zstd (default compression level 3)
    let compressed_data = zstd::stream::encode_all(Cursor::new(cbor_data), 3)
        .context("Failed to compress CBOR data with Zstd")?;

    Ok(compressed_data)
}

/// Decompresses Zstd data and deserializes it from CBOR back into the struct.
pub fn decompress_payload<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    // Decompress with Zstd
    let decompressed_data =
        zstd::stream::decode_all(Cursor::new(bytes)).context("Failed to decompress Zstd data")?;

    // Deserialize from CBOR
    let payload: T = ciborium::from_reader(Cursor::new(decompressed_data))
        .context("Failed to deserialize CBOR to struct")?;

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct DummyPayload {
        id: String,
        content: String,
        values: Vec<i32>,
    }

    #[test]
    fn test_compress_and_decompress_payload() {
        let original = DummyPayload {
            id: "doc_123".to_string(),
            content: "This is a test payload that will be serialized and compressed.".to_string(),
            values: vec![1, 2, 3, 4, 5],
        };

        // Compress
        let compressed_bytes = compress_payload(&original).expect("Should compress successfully");

        // Decompress
        let recovered: DummyPayload =
            decompress_payload(&compressed_bytes).expect("Should decompress successfully");

        assert_eq!(original, recovered);
    }
}
