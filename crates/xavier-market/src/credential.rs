use crate::model::Listing;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const CONTENT_DOMAIN: &str = "swal-listing-content-v1:";
const PROOF_DOMAIN: &str = "swal-listing-proof-v1:";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialError {
    #[error("Missing proof")]
    MissingProof,
    #[error("Unsupported proof type: {0}")]
    UnsupportedProof(String),
    #[error("Digest mismatch")]
    DigestMismatch,
    #[error("Invalid issuer")]
    InvalidIssuer,
    #[error("Invalid signature")]
    InvalidSignature,
}

#[rustfmt::skip]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListingSubject { pub id: String, pub listing_id: String, pub content_hash: String, pub license: String, pub price_credits: u64 }

#[rustfmt::skip]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListingProof { #[serde(rename = "type")] pub proof_type: String, pub created: String, pub verification_method: String, pub proof_purpose: String, pub proof_value: String }

#[rustfmt::skip]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListingCredential { #[serde(rename = "@context")] pub context: Vec<String>, pub id: String, #[serde(rename = "type")] pub credential_type: Vec<String>, pub issuer: String, pub valid_from: String, pub credential_subject: ListingSubject, #[serde(skip_serializing_if = "Option::is_none")] pub proof: Option<ListingProof> }

pub fn content_hash(listing: &Listing) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CONTENT_DOMAIN.as_bytes());
    hasher.update(
        format!(
            "{}|{}|{}|{}|{}",
            listing.id,
            listing.seller_id,
            listing.title,
            listing.license.as_str(),
            listing.price_credits
        )
        .as_bytes(),
    );
    hex::encode(hasher.finalize())
}

pub fn issue(listing: &Listing, key: &SigningKey, issued_at: &str) -> ListingCredential {
    let issuer_did = format!("did:swal:{}", hex::encode(key.verifying_key().as_bytes()));
    let subject = ListingSubject {
        id: format!("urn:swal:listing:{}", listing.id),
        listing_id: listing.id.clone(),
        content_hash: content_hash(listing),
        license: listing.license.as_str().to_string(),
        price_credits: listing.price_credits,
    };
    let mut vc = ListingCredential {
        context: vec![
            "https://www.w3.org/ns/credentials/v2".into(),
            "https://schema.org".into(),
        ],
        id: format!("urn:uuid:{}", Uuid::new_v4()),
        credential_type: vec!["VerifiableCredential".into(), "ListingCredential".into()],
        issuer: issuer_did.clone(),
        valid_from: issued_at.to_string(),
        credential_subject: subject,
        proof: None,
    };
    let mut hasher = Sha256::new();
    hasher.update(PROOF_DOMAIN.as_bytes());
    hasher.update(serde_json::to_string(&vc).unwrap().as_bytes());
    vc.proof = Some(ListingProof {
        proof_type: "Ed25519Signature2020".into(),
        created: issued_at.to_string(),
        verification_method: format!("{}#key-1", issuer_did),
        proof_purpose: "assertionMethod".into(),
        proof_value: hex::encode(key.sign(&hasher.finalize()).to_bytes()),
    });
    vc
}

pub fn verify(vc: &ListingCredential, listing: &Listing) -> Result<(), CredentialError> {
    let proof = vc.proof.as_ref().ok_or(CredentialError::MissingProof)?;
    if proof.proof_type != "Ed25519Signature2020" {
        return Err(CredentialError::UnsupportedProof(proof.proof_type.clone()));
    }
    if content_hash(listing) != vc.credential_subject.content_hash {
        return Err(CredentialError::DigestMismatch);
    }
    let hex_str = vc
        .issuer
        .strip_prefix("did:swal:")
        .unwrap_or(&vc.issuer)
        .split('#')
        .next()
        .unwrap();
    let pk_bytes = hex::decode(hex_str).map_err(|_| CredentialError::InvalidIssuer)?;
    let pk_arr: [u8; 32] = pk_bytes
        .try_into()
        .map_err(|_| CredentialError::InvalidIssuer)?;
    let verifying_key =
        VerifyingKey::from_bytes(&pk_arr).map_err(|_| CredentialError::InvalidIssuer)?;
    let sig_bytes =
        hex::decode(&proof.proof_value).map_err(|_| CredentialError::InvalidSignature)?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| CredentialError::InvalidSignature)?;
    let mut unproven_vc = vc.clone();
    unproven_vc.proof = None;
    let mut hasher = Sha256::new();
    hasher.update(PROOF_DOMAIN.as_bytes());
    hasher.update(serde_json::to_string(&unproven_vc).unwrap().as_bytes());
    verifying_key
        .verify(&hasher.finalize(), &Signature::from_bytes(&sig_arr))
        .map_err(|_| CredentialError::InvalidSignature)?;
    Ok(())
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*; use crate::config::License;
    fn get_listing() -> Listing { Listing { id: "l-1".into(), seller_id: "s-1".into(), title: "T".into(), license: License::SwalDataLicenseV0, price_credits: 100, created_at_ms: 1000 } }
    #[test] fn test_issue_verify() {
        let l = get_listing(); let k = SigningKey::from_bytes(&[7; 32]);
        assert!(verify(&issue(&l, &k, "2024-01-01T00:00:00Z"), &l).is_ok());
    }
    #[test] fn test_digest_mismatch() {
        let l = get_listing(); let k = SigningKey::from_bytes(&[7; 32]); let vc = issue(&l, &k, "2024-01-01T00:00:00Z");
        let mut cl = l.clone(); cl.price_credits = 200;
        assert_eq!(verify(&vc, &cl), Err(CredentialError::DigestMismatch));
    }
    #[test] fn test_invalid_sig_wrong_key() {
        let l = get_listing(); let k = SigningKey::from_bytes(&[7; 32]); let mut vc = issue(&l, &k, "2024-01-01T00:00:00Z");
        let mut u_vc = vc.clone(); u_vc.proof = None;
        let mut hasher = Sha256::new(); hasher.update(PROOF_DOMAIN.as_bytes()); hasher.update(serde_json::to_string(&u_vc).unwrap().as_bytes());
        vc.proof.as_mut().unwrap().proof_value = hex::encode(SigningKey::from_bytes(&[8; 32]).sign(&hasher.finalize()).to_bytes());
        assert_eq!(verify(&vc, &l), Err(CredentialError::InvalidSignature));
    }
    #[test] fn test_changed_proof() {
        let l = get_listing(); let k = SigningKey::from_bytes(&[7; 32]); let mut vc = issue(&l, &k, "2024-01-01T00:00:00Z");
        let mut sig_bytes = hex::decode(&vc.proof.as_ref().unwrap().proof_value).unwrap(); sig_bytes[0] ^= 1;
        vc.proof.as_mut().unwrap().proof_value = hex::encode(sig_bytes);
        assert_eq!(verify(&vc, &l), Err(CredentialError::InvalidSignature));
    }
    #[test] fn test_missing_proof() {
        let l = get_listing(); let k = SigningKey::from_bytes(&[7; 32]); let mut vc = issue(&l, &k, "2024-01-01T00:00:00Z"); vc.proof = None;
        assert_eq!(verify(&vc, &l), Err(CredentialError::MissingProof));
    }
}
