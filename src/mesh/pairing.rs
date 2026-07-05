use crate::mesh::node::NodeId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingCodeData {
    pub node_id: NodeId,
    pub endpoint: String,
    pub public_key_hex: String,
    pub secret: String,
    pub expires_at: u64,
}

pub fn generate_pairing_code(
    node_id: NodeId,
    endpoint: String,
    public_key_hex: String,
) -> (String, String) {
    let secret = uuid::Uuid::new_v4().to_string();
    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600; // 1 hour

    let data = PairingCodeData {
        node_id,
        endpoint,
        public_key_hex,
        secret: secret.clone(),
        expires_at,
    };

    let json = serde_json::to_string(&data).unwrap();
    (crate::crypto::base64_encode(json), secret)
}

pub fn decode_pairing_code(code: &str) -> Result<PairingCodeData> {
    let decoded = crate::crypto::base64_decode(code)
        .ok_or_else(|| anyhow::anyhow!("Failed to decode base64 pairing code"))?;
    let data: PairingCodeData =
        serde_json::from_slice(&decoded).context("Failed to parse pairing code JSON")?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if data.expires_at < now {
        anyhow::bail!("Pairing code has expired");
    }

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pairing_code_roundtrip() {
        let node_id = NodeId("xv1-testnode".to_string());
        let (code, secret) = generate_pairing_code(
            node_id.clone(),
            "http://localhost:8006".to_string(),
            "aabbccdd".to_string(),
        );

        assert!(!code.is_empty());
        assert_eq!(secret.len(), 36); // UUID length

        let decoded = decode_pairing_code(&code).unwrap();
        assert_eq!(decoded.node_id, node_id);
        assert_eq!(decoded.endpoint, "http://localhost:8006");
        assert_eq!(decoded.public_key_hex, "aabbccdd");
        assert_eq!(decoded.secret, secret);
        assert!(decoded.expires_at > 0);
    }

    #[test]
    fn test_pairing_code_expiration() {
        let data = PairingCodeData {
            node_id: NodeId("test".to_string()),
            endpoint: "test".to_string(),
            public_key_hex: "test".to_string(),
            secret: "test".to_string(),
            expires_at: 1000, // Long ago
        };

        let json = serde_json::to_string(&data).unwrap();
        let code = crate::crypto::base64_encode(json);

        let result = decode_pairing_code(&code);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Pairing code has expired");
    }
}
