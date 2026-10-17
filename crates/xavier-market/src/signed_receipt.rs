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
    #[error("unknown or mismatched issuer: {0}")]
    UnknownIssuer(String),
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid signature format: {0}")]
    InvalidSignatureFormat(String),
    #[error("invalid signature")]
    InvalidSignature,
}

/// Computes the domain-separated canonical bytes for a receipt.
/// Note: Serialization depends on the field order of `Receipt` derived by serde.
/// Reordering fields in `Receipt` invalidates existing signatures.
pub fn canonical_bytes(receipt: &Receipt) -> Vec<u8> {
    let mut bytes = RECEIPT_DOMAIN.to_vec();
    bytes.extend(serde_json::to_vec(receipt).expect("Receipt serializes"));
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

pub fn verify(
    signed: &SignedReceipt,
    expected_issuer: &VerifyingKey,
) -> Result<(), ReceiptSignatureError> {
    let key_bytes = hex::decode(&signed.issuer_key_hex)
        .map_err(|e| ReceiptSignatureError::InvalidKey(e.to_string()))?;

    if key_bytes.len() != 32 {
        return Err(ReceiptSignatureError::InvalidKey(format!(
            "Expected 32 bytes for Ed25519 key, got {}",
            key_bytes.len()
        )));
    }

    if key_bytes.as_slice() != expected_issuer.as_bytes() {
        return Err(ReceiptSignatureError::UnknownIssuer(format!(
            "Receipt issuer key {} does not match expected issuer key {}",
            signed.issuer_key_hex,
            hex::encode(expected_issuer.as_bytes())
        )));
    }

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
    expected_issuer
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
            currency: Currency::InternalCredits,
            created_at_ms: 1234567890,
        }
    }

    #[test]
    fn test_sign_and_verify_success() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let signed = sign(receipt, &key);

        assert!(verify(&signed, &key.verifying_key()).is_ok());
    }

    #[test]
    fn test_verify_changed_amount_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        signed.receipt.amount_credits = 100;

        assert!(matches!(
            verify(&signed, &key.verifying_key()),
            Err(ReceiptSignatureError::InvalidSignature)
        ));
    }

    #[test]
    fn test_verify_forged_receipt_by_attacker_key_fails() {
        let platform_key = get_test_key();
        let attacker_key = SigningKey::from_bytes(&[7u8; 32]);

        // Attacker creates and signs a receipt with their own key
        let receipt = get_test_receipt();
        let forged = sign(receipt, &attacker_key);

        // Verification against platform verifying key must fail with UnknownIssuer
        let result = verify(&forged, &platform_key.verifying_key());
        assert!(matches!(
            result,
            Err(ReceiptSignatureError::UnknownIssuer(_))
        ));
    }

    #[test]
    fn test_verify_tampered_issuer_hex_fails() {
        let key1 = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key1);

        let secret2 = [8u8; 32];
        let key2 = SigningKey::from_bytes(&secret2);

        // Tamper with issuer_key_hex to a different key
        signed.issuer_key_hex = hex::encode(key2.verifying_key().as_bytes());

        assert!(matches!(
            verify(&signed, &key1.verifying_key()),
            Err(ReceiptSignatureError::UnknownIssuer(_))
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
            verify(&signed, &key.verifying_key()),
            Err(ReceiptSignatureError::InvalidSignatureFormat(_))
        ));
    }

    #[test]
    fn test_verify_invalid_hex_key_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        signed.issuer_key_hex = "not_valid_hex_string!".to_string();

        assert!(matches!(
            verify(&signed, &key.verifying_key()),
            Err(ReceiptSignatureError::InvalidKey(_))
        ));
    }

    #[test]
    fn test_verify_short_key_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        // 16 bytes instead of 32
        signed.issuer_key_hex = hex::encode([1u8; 16]);

        assert!(matches!(
            verify(&signed, &key.verifying_key()),
            Err(ReceiptSignatureError::InvalidKey(_))
        ));
    }

    #[test]
    fn test_verify_invalid_hex_signature_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        signed.signature_hex = "zz".repeat(64);

        assert!(matches!(
            verify(&signed, &key.verifying_key()),
            Err(ReceiptSignatureError::InvalidSignatureFormat(_))
        ));
    }

    #[test]
    fn test_verify_flipped_byte_in_full_signature_fails() {
        let key = get_test_key();
        let receipt = get_test_receipt();
        let mut signed = sign(receipt, &key);

        let mut sig_bytes = hex::decode(&signed.signature_hex).unwrap();
        // Flip one bit in the valid 64-byte signature
        sig_bytes[0] ^= 0x01;
        signed.signature_hex = hex::encode(sig_bytes);

        assert!(matches!(
            verify(&signed, &key.verifying_key()),
            Err(ReceiptSignatureError::InvalidSignature)
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
