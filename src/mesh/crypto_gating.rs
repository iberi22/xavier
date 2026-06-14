use serde::{Deserialize, Serialize};

/// Simplified mock representation of Token Gating and Symmetric Encryption for Data Commons.
/// In production, this uses real Ed25519 signatures and AES-GCM for encryption.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EncryptedPayload {
    pub cipher_text: String,
    pub nonce: String,
    pub keychain_address: String, // The smart contract or master wallet that holds the key
    pub ipfs_cid: String, // The PIN/hash used to retrieve the payload from the decentralized network
}

/// Request sent by a Mantainer to access an encrypted log.
#[derive(Debug, Deserialize)]
pub struct AccessRequest {
    pub wallet_address: String,
    pub signature: String, // Proof of ownership of the wallet
}

/// Service that handles token-gating and symmetric key distribution.
pub struct CryptoGatingService {
    mock_symmetric_key: String,
}

impl CryptoGatingService {
    pub fn new() -> Self {
        Self {
            mock_symmetric_key: "0xXAVIER_SECRET_SYMMETRIC_KEY".to_string(),
        }
    }

    /// Encrypts a serialized payload locally before sending to the Mesh.
    pub fn encrypt_payload(&self, raw_json: &str) -> EncryptedPayload {
        // MOCK: In production use AES-GCM.
        let hex_encoded = hex::encode(raw_json);
        
        // Generate a mock IPFS Content ID (CID) PIN based on the payload hash
        let mock_cid = format!("Qm{}", hex::encode(&hex_encoded[0..std::cmp::min(10, hex_encoded.len())]));
        
        EncryptedPayload {
            cipher_text: hex_encoded,
            nonce: "mock-nonce".to_string(),
            keychain_address: "XAV-DAO-CONTRACT".to_string(),
            ipfs_cid: mock_cid,
        }
    }

    /// Decrypts a payload if the provided symmetric key matches.
    pub fn decrypt_payload(&self, payload: &EncryptedPayload, provided_key: &str) -> Result<String, String> {
        if provided_key != self.mock_symmetric_key {
            return Err("Invalid symmetric key".to_string());
        }

        // MOCK: In production use AES-GCM.
        let decoded = hex::decode(&payload.cipher_text)
            .map_err(|e| format!("Hex decode error: {}", e))?;
        
        String::from_utf8(decoded)
            .map_err(|e| format!("UTF8 error: {}", e))
    }

    /// Validates if a Wallet Address has paid the XAV token toll, or is a whitelisted maintainer.
    pub fn validate_access(&self, request: &AccessRequest) -> Result<String, String> {
        // MOCK: In production, verify Ed25519 signature of the request, 
        // then call Solana RPC or Supabase cached state to verify token balances.
        
        // For tests, we accept wallets starting with "MAINTAINER_" and valid signatures.
        if !request.wallet_address.starts_with("MAINTAINER_") {
            return Err("Access denied: Wallet does not hold XAV tokens or maintainer pass".to_string());
        }
        if request.signature != "valid_signature" {
            return Err("Access denied: Invalid signature".to_string());
        }

        // Return the symmetric key
        Ok(self.mock_symmetric_key.clone())
    }
}

impl Default for CryptoGatingService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_decryption() {
        let service = CryptoGatingService::new();
        let payload = service.encrypt_payload("{\"test\": 1}");
        
        let decrypted = service.decrypt_payload(&payload, "0xXAVIER_SECRET_SYMMETRIC_KEY").unwrap();
        assert_eq!(decrypted, "{\"test\": 1}");

        let fail = service.decrypt_payload(&payload, "wrong_key");
        assert!(fail.is_err());
    }

    #[test]
    fn test_access_validation() {
        let service = CryptoGatingService::new();
        
        // Valid maintainer
        let req1 = AccessRequest {
            wallet_address: "MAINTAINER_123".to_string(),
            signature: "valid_signature".to_string(),
        };
        assert!(service.validate_access(&req1).is_ok());

        // Invalid signature
        let req2 = AccessRequest {
            wallet_address: "MAINTAINER_123".to_string(),
            signature: "bad_sig".to_string(),
        };
        assert!(service.validate_access(&req2).is_err());

        // Not a maintainer
        let req3 = AccessRequest {
            wallet_address: "FREE_USER_ABC".to_string(),
            signature: "valid_signature".to_string(),
        };
        assert!(service.validate_access(&req3).is_err());
    }
}
