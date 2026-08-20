//! # W3C Verifiable Credentials for SWAL Data Commons
//!
//! Generates and verifies cryptographic provenance certificates for curated datasets using
//! Ed25519 signatures and W3C Verifiable Credentials 2.0 / JSON-LD standard structure.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

use crate::crypto::{hex_decode, hex_encode};

/// Errors that can occur during credential generation, verification, or parsing.
#[derive(Debug)]
pub enum CredentialError {
    /// Cryptographic signature verification failed.
    InvalidSignature,
    /// Dataset hash digest mismatch indicating dataset corruption or tampering.
    DigestMismatch { expected: String, actual: String },
    /// Invalid or malformed public key.
    InvalidPublicKey(String),
    /// Invalid signature formatting (e.g. invalid hex or size).
    InvalidSignatureFormat(String),
    /// Serialization or deserialization error.
    SerializationError(String),
    /// Credential or proof formatting error.
    MalformedCredential(String),
}

impl std::error::Error for CredentialError {}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialError::InvalidSignature => write!(f, "Cryptographic signature verification failed"),
            CredentialError::DigestMismatch { expected, actual } => {
                write!(f, "Dataset digest mismatch: expected {expected}, got {actual}")
            }
            CredentialError::InvalidPublicKey(msg) => write!(f, "Invalid public key: {msg}"),
            CredentialError::InvalidSignatureFormat(msg) => write!(f, "Invalid signature format: {msg}"),
            CredentialError::SerializationError(msg) => write!(f, "Serialization error: {msg}"),
            CredentialError::MalformedCredential(msg) => write!(f, "Malformed credential: {msg}"),
        }
    }
}

/// W3C Verifiable Credential Subject representing training dataset provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DatasetCredentialSubject {
    /// Subject identifier URI (e.g. `urn:swal:dataset:<id>`)
    pub id: String,
    /// Canonical dataset identifier.
    pub dataset_id: String,
    /// Human-readable dataset name.
    pub dataset_name: String,
    /// SHA-256 digest of raw dataset content (64-char lower hex).
    pub dataset_digest: String,
    /// Number of curated records in the dataset bundle.
    pub record_count: usize,
    /// License under which the dataset is published.
    pub license: String,
    /// Curation status (e.g. "APPROVED", "CURATED").
    pub curation_status: String,
}

/// Cryptographic Proof embedded in W3C Verifiable Credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialProof {
    /// Proof suite type (e.g. "Ed25519Signature2020").
    #[serde(rename = "type")]
    pub proof_type: String,
    /// ISO-8601 / RFC-3339 creation timestamp.
    pub created: String,
    /// Verification method URI (e.g., `did:swal:<pubkey_hex>#key-1`).
    pub verification_method: String,
    /// Proof purpose (e.g., "assertionMethod").
    pub proof_purpose: String,
    /// Hex-encoded Ed25519 signature payload.
    pub proof_value: String,
}

/// W3C Verifiable Credential 2.0 structure for Training Datasets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifiableCredential {
    /// JSON-LD context references.
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    /// Unique credential identifier URN.
    pub id: String,
    /// Credential types array (must include "VerifiableCredential").
    #[serde(rename = "type")]
    pub credential_type: Vec<String>,
    /// Issuer Decentralized Identifier (DID).
    pub issuer: String,
    /// Issuance timestamp in ISO-8601 / RFC-3339 format.
    pub valid_from: String,
    /// Subject containing dataset claims.
    pub credential_subject: DatasetCredentialSubject,
    /// Cryptographic proof attached to the credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<CredentialProof>,
}

/// Parameters required to construct a dataset credential.
#[derive(Debug, Clone)]
pub struct DatasetCredentialParams {
    /// Dataset ID.
    pub dataset_id: String,
    /// Dataset human-readable name.
    pub dataset_name: String,
    /// Raw dataset bytes for computing SHA-256 digest (or pre-computed digest).
    pub dataset_digest: String,
    /// Total record count.
    pub record_count: usize,
    /// License string (default: "AGPL-3.0-only" or "CC-BY-4.0").
    pub license: String,
    /// Curation status.
    pub curation_status: String,
}

/// Generator and verifier for W3C Verifiable Credentials with Ed25519 signatures.
pub struct DatasetCredentialGenerator;

impl DatasetCredentialGenerator {
    /// Context URLs for W3C Verifiable Credentials 2.0
    pub fn default_context() -> Vec<String> {
        vec![
            "https://www.w3.org/ns/credentials/v2".to_string(),
            "https://schema.org".to_string(),
        ]
    }

    /// Converts an Ed25519 verifying key into a canonical DID string (`did:swal:<pubkey_hex>`).
    pub fn public_key_to_did(public_key: &VerifyingKey) -> String {
        let hex_pk = hex_encode(public_key.as_bytes());
        format!("did:swal:{hex_pk}")
    }

    /// Extracts `VerifyingKey` from a `did:swal:<pubkey_hex>` string.
    pub fn did_to_public_key(did: &str) -> Result<VerifyingKey, CredentialError> {
        let prefix = "did:swal:";
        let hex_str = if let Some(stripped) = did.strip_prefix(prefix) {
            stripped.split('#').next().unwrap_or(stripped)
        } else {
            did.split('#').next().unwrap_or(did)
        };

        let bytes = hex_decode(hex_str).map_err(|e| CredentialError::InvalidPublicKey(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(CredentialError::InvalidPublicKey(format!(
                "Expected 32 bytes for Ed25519 key, got {}",
                bytes.len()
            )));
        }

        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CredentialError::InvalidPublicKey("Invalid key slice conversion".to_string()))?;

        VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| CredentialError::InvalidPublicKey(format!("Ed25519 key parse error: {e}")))
    }

    /// Computes SHA-256 hex digest of raw byte content.
    pub fn compute_dataset_digest(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        hex_encode(hasher.finalize())
    }

    /// Generates a signed W3C Verifiable Credential for a dataset.
    pub fn generate_credential(
        params: DatasetCredentialParams,
        signing_key: &SigningKey,
    ) -> Result<VerifiableCredential, CredentialError> {
        let verifying_key = signing_key.verifying_key();
        let issuer_did = Self::public_key_to_did(&verifying_key);
        let credential_id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
        let created_timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let subject_id = format!("urn:swal:dataset:{}", params.dataset_id);

        let subject = DatasetCredentialSubject {
            id: subject_id,
            dataset_id: params.dataset_id,
            dataset_name: params.dataset_name,
            dataset_digest: params.dataset_digest,
            record_count: params.record_count,
            license: params.license,
            curation_status: params.curation_status,
        };

        let mut vc = VerifiableCredential {
            context: Self::default_context(),
            id: credential_id,
            credential_type: vec![
                "VerifiableCredential".to_string(),
                "DatasetCredential".to_string(),
            ],
            issuer: issuer_did.clone(),
            valid_from: created_timestamp.clone(),
            credential_subject: subject,
            proof: None,
        };

        let canonical_bytes = Self::canonical_signing_bytes(&vc)?;
        let signature: Signature = signing_key.sign(&canonical_bytes);
        let proof_value = hex_encode(signature.to_bytes());

        let proof = CredentialProof {
            proof_type: "Ed25519Signature2020".to_string(),
            created: created_timestamp,
            verification_method: format!("{issuer_did}#key-1"),
            proof_purpose: "assertionMethod".to_string(),
            proof_value,
        };

        vc.proof = Some(proof);
        Ok(vc)
    }

    /// Verifies signature, issuer identity, and structural integrity of a Verifiable Credential.
    /// If `expected_content` is provided, also verifies that dataset content matches `dataset_digest`.
    pub fn verify_credential(
        vc: &VerifiableCredential,
        expected_content: Option<&[u8]>,
    ) -> Result<bool, CredentialError> {
        let proof = vc
            .proof
            .as_ref()
            .ok_ok_or_malformed("Missing proof in Verifiable Credential")?;

        if proof.proof_type != "Ed25519Signature2020" {
            return Err(CredentialError::MalformedCredential(format!(
                "Unsupported proof type: {}",
                proof.proof_type
            )));
        }

        // Verify dataset digest if raw content provided
        if let Some(content) = expected_content {
            let actual_digest = Self::compute_dataset_digest(content);
            if actual_digest != vc.credential_subject.dataset_digest {
                return Err(CredentialError::DigestMismatch {
                    expected: vc.credential_subject.dataset_digest.clone(),
                    actual: actual_digest,
                });
            }
        }

        let verifying_key = Self::did_to_public_key(&vc.issuer)?;
        let canonical_bytes = Self::canonical_signing_bytes(vc)?;

        let sig_bytes = hex_decode(&proof.proof_value)
            .map_err(|e| CredentialError::InvalidSignatureFormat(e.to_string()))?;

        if sig_bytes.len() != 64 {
            return Err(CredentialError::InvalidSignatureFormat(format!(
                "Expected 64 bytes for Ed25519 signature, got {}",
                sig_bytes.len()
            )));
        }

        let signature_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| CredentialError::InvalidSignatureFormat("Invalid signature slice".to_string()))?;

        let signature = Signature::from_bytes(&signature_arr);

        verifying_key
            .verify(&canonical_bytes, &signature)
            .map_err(|_| CredentialError::InvalidSignature)?;

        Ok(true)
    }

    /// Constructs deterministic canonical signing payload bytes for the credential.
    fn canonical_signing_bytes(vc: &VerifiableCredential) -> Result<Vec<u8>, CredentialError> {
        // Strip proof field for signing payload computation
        let mut unproven_vc = vc.clone();
        unproven_vc.proof = None;

        let json_str = serde_json::to_string(&unproven_vc)
            .map_err(|e| CredentialError::SerializationError(e.to_string()))?;

        let mut hasher = Sha256::new();
        hasher.update(b"swal-vc-proof-v1:");
        hasher.update(json_str.as_bytes());

        Ok(hasher.finalize().to_vec())
    }
}

trait OptionExt<T> {
    fn ok_ok_or_malformed(self, msg: &'static str) -> Result<T, CredentialError>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_ok_or_malformed(self, msg: &'static str) -> Result<T, CredentialError> {
        self.ok_or_else(|| CredentialError::MalformedCredential(msg.to_string()))
    }
}
