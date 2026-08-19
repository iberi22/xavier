//! Node certificate issuance and cryptographic verification
//!
//! A node certificate guarantees cryptographic isolation between wallets.
//! It consists of a wallet signature (Ed25519) over:
//! `(wallet_pubkey || node_pubkey || node_id || expiry)`.

use crate::utils::crypto::{hex_decode, hex_encode};
use anyhow::{anyhow, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const CERT_DOMAIN_PREFIX: &[u8] = b"swal-node-cert-v1:";

/// Cryptographic certificate binding a node to a wallet authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeCertificate {
    pub wallet_pubkey: String,
    pub node_pubkey: String,
    pub node_id: String,
    pub expiry: u64,
    pub signature: String,
}

impl NodeCertificate {
    /// Constructs the canonical payload bytes that are signed.
    pub fn payload_bytes(
        wallet_pubkey: &str,
        node_pubkey: &str,
        node_id: &str,
        expiry: u64,
    ) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(128);
        bytes.extend_from_slice(CERT_DOMAIN_PREFIX);
        bytes.extend_from_slice(wallet_pubkey.as_bytes());
        bytes.push(b':');
        bytes.extend_from_slice(node_pubkey.as_bytes());
        bytes.push(b':');
        bytes.extend_from_slice(node_id.as_bytes());
        bytes.push(b':');
        bytes.extend_from_slice(&expiry.to_le_bytes());
        bytes
    }

    /// Check if the certificate is currently expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expiry
    }
}

/// Issue a new node certificate signed by the user's wallet private key.
pub fn issue_cert(
    wallet_signing_key: &SigningKey,
    node_pubkey_bytes: &[u8; 32],
    node_id: &str,
    ttl_secs: u64,
) -> Result<NodeCertificate> {
    let wallet_vk = wallet_signing_key.verifying_key();
    let wallet_pubkey_hex = hex_encode(wallet_vk.as_bytes());
    let node_pubkey_hex = hex_encode(node_pubkey_bytes);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("System clock is before UNIX epoch")?
        .as_secs();
    let expiry = now.saturating_add(ttl_secs);

    let payload =
        NodeCertificate::payload_bytes(&wallet_pubkey_hex, &node_pubkey_hex, node_id, expiry);

    let signature = wallet_signing_key.sign(&payload);
    let signature_hex = hex_encode(&signature.to_bytes());

    Ok(NodeCertificate {
        wallet_pubkey: wallet_pubkey_hex,
        node_pubkey: node_pubkey_hex,
        node_id: node_id.to_string(),
        expiry,
        signature: signature_hex,
    })
}

/// Verify a node certificate's signature, expiry, and optional expected wallet public key.
pub fn verify_cert(
    cert: &NodeCertificate,
    expected_wallet_pubkey: Option<&[u8; 32]>,
) -> Result<bool> {
    if cert.is_expired() {
        return Ok(false);
    }

    let wallet_pk_bytes = hex_decode(&cert.wallet_pubkey)
        .map_err(|e| anyhow!("Invalid hex in certificate wallet_pubkey: {}", e))?;
    if wallet_pk_bytes.len() != 32 {
        return Ok(false);
    }

    let mut pk_arr = [0u8; 32];
    pk_arr.copy_from_slice(&wallet_pk_bytes);

    if let Some(expected) = expected_wallet_pubkey {
        if &pk_arr != expected {
            return Ok(false);
        }
    }

    let sig_bytes = hex_decode(&cert.signature)
        .map_err(|e| anyhow!("Invalid hex in certificate signature: {}", e))?;
    if sig_bytes.len() != 64 {
        return Ok(false);
    }

    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = Signature::from_bytes(&sig_arr);

    let verifying_key = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|e| anyhow!("Invalid Ed25519 public key in certificate: {}", e))?;

    let payload = NodeCertificate::payload_bytes(
        &cert.wallet_pubkey,
        &cert.node_pubkey,
        &cert.node_id,
        cert.expiry,
    );

    match verifying_key.verify(&payload, &signature) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_issue_and_verify_valid_certificate() {
        let wallet_sk = SigningKey::generate(&mut OsRng);
        let wallet_pk = wallet_sk.verifying_key().to_bytes();

        let node_sk = SigningKey::generate(&mut OsRng);
        let node_pk = node_sk.verifying_key().to_bytes();
        let node_id = "xv1-testnode12345";

        let cert = issue_cert(&wallet_sk, &node_pk, node_id, 3600).unwrap();
        assert_eq!(cert.node_id, node_id);
        assert!(!cert.is_expired());

        let valid = verify_cert(&cert, Some(&wallet_pk)).unwrap();
        assert!(valid, "Certificate must verify with correct wallet key");

        let valid_no_expected = verify_cert(&cert, None).unwrap();
        assert!(
            valid_no_expected,
            "Certificate must verify against self-contained wallet key"
        );
    }

    #[test]
    fn test_reject_certificate_from_different_wallet() {
        let wallet1_sk = SigningKey::generate(&mut OsRng);
        let wallet2_sk = SigningKey::generate(&mut OsRng);
        let wallet2_pk = wallet2_sk.verifying_key().to_bytes();

        let node_sk = SigningKey::generate(&mut OsRng);
        let node_pk = node_sk.verifying_key().to_bytes();
        let node_id = "xv1-testnode12345";

        let cert = issue_cert(&wallet1_sk, &node_pk, node_id, 3600).unwrap();

        // Verification against wallet2's expected pubkey must fail
        let valid = verify_cert(&cert, Some(&wallet2_pk)).unwrap();
        assert!(
            !valid,
            "Certificate from wallet 1 must not verify against wallet 2"
        );
    }

    #[test]
    fn test_reject_tampered_certificate() {
        let wallet_sk = SigningKey::generate(&mut OsRng);
        let wallet_pk = wallet_sk.verifying_key().to_bytes();

        let node_sk = SigningKey::generate(&mut OsRng);
        let node_pk = node_sk.verifying_key().to_bytes();
        let node_id = "xv1-testnode12345";

        let mut cert = issue_cert(&wallet_sk, &node_pk, node_id, 3600).unwrap();
        cert.node_id = "xv1-tamperednode".to_string();

        let valid = verify_cert(&cert, Some(&wallet_pk)).unwrap();
        assert!(
            !valid,
            "Tampered certificate payload must fail verification"
        );
    }

    #[test]
    fn test_expired_certificate() {
        let wallet_sk = SigningKey::generate(&mut OsRng);
        let wallet_pk = wallet_sk.verifying_key().to_bytes();

        let node_sk = SigningKey::generate(&mut OsRng);
        let node_pk = node_sk.verifying_key().to_bytes();
        let node_id = "xv1-testnode12345";

        // Expired in past
        let mut cert = issue_cert(&wallet_sk, &node_pk, node_id, 0).unwrap();
        cert.expiry = 1000; // Ancient timestamp

        let valid = verify_cert(&cert, Some(&wallet_pk)).unwrap();
        assert!(!valid, "Expired certificate must be rejected");
    }
}
