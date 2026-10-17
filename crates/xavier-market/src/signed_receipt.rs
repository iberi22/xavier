use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::Receipt;

const RECEIPT_DOMAIN: &[u8] = b"swal-market-receipt-v1:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedReceipt {
    pub receipt: Receipt,
    pub issuer_key_hex: String,
    pub signature_hex: String,
}

#[derive(Debug, Error)]
pub enum ReceiptSignatureError {
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid signature format: {0}")]
    InvalidSignatureFormat(String),
    #[error("invalid signature")]
    InvalidSignature,
}

pub fn canonical_bytes(receipt: &Receipt) -> Vec<u8> {
    let mut bytes = RECEIPT_DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(receipt).unwrap());
    bytes
}

pub fn sign(receipt: Receipt, key: &SigningKey) -> SignedReceipt {
    let canonical = canonical_bytes(&receipt);
    let signature = key.sign(&canonical);
    let issuer_key_hex = hex::encode(key.verifying_key().as_bytes());
    let signature_hex = hex::encode(signature.to_bytes());

    SignedReceipt {
        receipt,
        issuer_key_hex,
        signature_hex,
    }
}

pub fn verify(signed: &SignedReceipt) -> Result<(), ReceiptSignatureError> {
    let key_bytes = hex::decode(&signed.issuer_key_hex)
        .map_err(|e| ReceiptSignatureError::InvalidKey(e.to_string()))?;

    if key_bytes.len() != 32 {
        return Err(ReceiptSignatureError::InvalidKey(format!(
            "Expected 32 bytes for Ed25519 key, got {}",
            key_bytes.len()
        )));
    }

    let key_arr: [u8; 32] = key_bytes.try_into().map_err(|_| {
        ReceiptSignatureError::InvalidKey("Invalid key slice conversion".to_string())
    })?;

    let verifying_key = VerifyingKey::from_bytes(&key_arr)
        .map_err(|e| ReceiptSignatureError::InvalidKey(format!("Ed25519 key parse error: {e}")))?;

    let sig_bytes = hex::decode(&signed.signature_hex)
        .map_err(|e| ReceiptSignatureError::InvalidSignatureFormat(e.to_string()))?;

    if sig_bytes.len() != 64 {
        return Err(ReceiptSignatureError::InvalidSignatureFormat(format!(
            "Expected 64 bytes for Ed25519 signature, got {}",
            sig_bytes.len()
        )));
    }

    let sig_arr: [u8; 64] = sig_bytes.try_into().map_err(|_| {
        ReceiptSignatureError::InvalidSignatureFormat("Invalid signature slice".to_string())
    })?;

    let signature = Signature::from_bytes(&sig_arr);

    let canonical = canonical_bytes(&signed.receipt);
    verifying_key
        .verify(&canonical, &signature)
        .map_err(|_| ReceiptSignatureError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Currency;

    fn get_test_key() -> SigningKey {
        let secret = [9u8; 32];
        SigningKey::from_bytes(&secret)
    }

    fn get_test_receipt() -> Receipt {
        Receipt {
            id: "rec_123".to_string(),
            listing_id: "lst_456".to_string(),
            buyer_id: "buy_789".to_string(),
            amount_credits: 50,
            currency: Currency::Credits,
            created_at_ms: 1234567890,
        }
    }

    #[test]
    fn test_sign_and_verify_success() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let signed = sign(receipt, &key);

        assert!(verify(&signed).is_ok());
    }

    #[test]
    fn test_verify_changed_amount_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        signed.receipt.amount_credits = 100;

        assert!(matches!(
            verify(&signed),
            Err(ReceiptSignatureError::InvalidSignature)
        ));
    }

    #[test]
    fn test_verify_different_key_fails() {
        let key1 = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key1);

        let secret2 = [8u8; 32];
        let key2 = SigningKey::from_bytes(&secret2);

        signed.issuer_key_hex = hex::encode(key2.verifying_key().as_bytes());

        assert!(matches!(
            verify(&signed),
            Err(ReceiptSignatureError::InvalidSignature)
        ));
    }

    #[test]
    fn test_verify_truncated_signature_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        // Truncate hex signature
        signed.signature_hex.truncate(64);

        assert!(matches!(
            verify(&signed),
            Err(ReceiptSignatureError::InvalidSignatureFormat(_))
        ));
    }

    #[test]
    fn test_canonical_bytes_deterministic() {
        let receipt1 = get_test_receipt();
        let receipt2 = get_test_receipt();

        let bytes1 = canonical_bytes(&receipt1);
        let bytes2 = canonical_bytes(&receipt2);

        assert_eq!(bytes1, bytes2);
    }
}
