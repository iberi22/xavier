//! Metadata redaction helper for MCP output.
//!
//! Recursively drops internal storage and cryptographic fields before
//! metadata is returned to MCP clients.

use serde_json::Value;

/// Returns true if a metadata key is an internal storage or crypto field that
/// must be redacted from public output.
fn is_redacted_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower == "encrypted_dek"
        || lower == "content_iv"
        || lower == "nonce"
        || lower.starts_with("encrypted_")
        || lower.ends_with("_dek")
        || lower.ends_with("_iv")
        || lower.ends_with("_nonce")
        || lower.ends_with("_key_id")
}

/// Recursively removes internal storage and crypto fields from a JSON metadata value.
pub fn public_metadata(meta: &Value) -> Value {
    match meta {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| !is_redacted_key(k))
                .map(|(k, v)| (k.clone(), public_metadata(v)))
                .collect();
            Value::Object(filtered)
        }
        Value::Array(arr) => {
            let filtered: Vec<Value> = arr.iter().map(public_metadata).collect();
            Value::Array(filtered)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_public_metadata_drops_top_level_crypto_keys() {
        let input = json!({
            "title": "Document 1",
            "encrypted_dek": "secret-dek",
            "content_iv": "secret-iv",
            "nonce": "secret-nonce",
            "encrypted_payload": "secret-payload",
            "custom_dek": "secret-dek-2",
            "chunk_iv": "secret-iv-2",
            "request_nonce": "secret-nonce-2",
            "master_key_id": "secret-key-id",
            "author": "Alice",
            "tags": ["tag1", "tag2"]
        });

        let output = public_metadata(&input);
        assert_eq!(
            output,
            json!({
                "title": "Document 1",
                "author": "Alice",
                "tags": ["tag1", "tag2"]
            })
        );
    }

    #[test]
    fn test_public_metadata_nested_objects_and_arrays() {
        let input = json!({
            "nested": {
                "safe": "yes",
                "encrypted_dek": "drop-me",
                "deep": {
                    "content_iv": "drop-me-too",
                    "value": 42
                }
            },
            "array": [
                {
                    "item": 1,
                    "nonce": "drop-nonce"
                },
                {
                    "item": 2,
                    "custom_key_id": "drop-key-id"
                }
            ]
        });

        let output = public_metadata(&input);
        assert_eq!(
            output,
            json!({
                "nested": {
                    "safe": "yes",
                    "deep": {
                        "value": 42
                    }
                },
                "array": [
                    {
                        "item": 1
                    },
                    {
                        "item": 2
                    }
                ]
            })
        );
    }

    #[test]
    fn test_public_metadata_primitives() {
        assert_eq!(public_metadata(&json!("hello")), json!("hello"));
        assert_eq!(public_metadata(&json!(123)), json!(123));
        assert_eq!(public_metadata(&json!(true)), json!(true));
        assert_eq!(public_metadata(&json!(null)), json!(null));
    }
}
