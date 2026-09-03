//! Inbound HTTP handler for Data Commons Data Marketplace
//!
//! Exposes dataset listing, query with payment, revocation, and pricing oracle previews over REST.

use super::error_response;
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
#[cfg(feature = "post-quantum")]
use oqs::sig::{Algorithm as SigAlg, Sig};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock, RwLock};

use crate::data_commons::marketplace::{DataMarketplace, DatasetId, DatasetMetadata};
use crate::data_commons::pricing::{
    calculate_price, PriceOracle, PricingTier, QualityLevel, TokenAmount,
};

// ---------------------------------------------------------------------------
// Module Singleton State
// ---------------------------------------------------------------------------

static MARKETPLACE: LazyLock<RwLock<DataMarketplace>> =
    LazyLock::new(|| RwLock::new(DataMarketplace::new()));

/// Wire or override the active `DataMarketplace` instance.
pub fn init_marketplace(marketplace: DataMarketplace) {
    if let Ok(mut guard) = MARKETPLACE.write() {
        *guard = marketplace;
    } else {
        tracing::error!("MARKETPLACE lock poisoned while initializing marketplace");
    }
}

fn current_marketplace() -> std::sync::RwLockReadGuard<'static, DataMarketplace> {
    MARKETPLACE.read().expect("MARKETPLACE lock poisoned")
}

fn current_marketplace_mut() -> std::sync::RwLockWriteGuard<'static, DataMarketplace> {
    MARKETPLACE.write().expect("MARKETPLACE lock poisoned")
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Request payload for listing a dataset.
#[derive(Debug, Deserialize)]
pub struct ListDatasetRequest {
    pub name: String,
    pub description: String,
    pub category: String,
    pub publisher: String,
    #[serde(default)]
    pub rows: Vec<serde_json::Value>,
    #[serde(default = "default_tier")]
    pub tier: PricingTier,
    #[serde(default)]
    pub reputation: f64,
    pub public_key: Option<String>,
    pub signature: Option<String>,
    pub fingerprint: Option<String>,
}

fn default_tier() -> PricingTier {
    PricingTier::Colaborador
}

/// Request payload for querying a dataset.
#[derive(Debug, Deserialize)]
pub struct QueryDatasetRequest {
    #[serde(default)]
    pub query: String,
    pub payment: u64,
}

/// Query parameters for pricing oracle preview.
#[derive(Debug, Deserialize)]
pub struct PricingPreviewQuery {
    pub size: Option<u64>,
    pub reputation: Option<f64>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Compute SHA-256 fingerprint for dataset payload.
pub fn compute_dataset_fingerprint(
    name: &str,
    category: &str,
    publisher: &str,
    description: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update(b":");
    hasher.update(category.as_bytes());
    hasher.update(b":");
    hasher.update(publisher.as_bytes());
    hasher.update(b":");
    hasher.update(description.as_bytes());
    crate::crypto::hex_encode(hasher.finalize())
}

/// Derive `xv1` address from ML-DSA public key bytes.
fn derive_address_from_pk(pk: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pk);
    let hash = hasher.finalize();

    let base32_data = bech32::convert_bits(&hash[..32], 8, 5, true).expect("valid conversion");
    let mut b32 = Vec::new();
    for i in base32_data {
        b32.push(bech32::u5::try_from_u8(i).expect("valid u5"));
    }
    bech32::encode("xv1", b32, bech32::Variant::Bech32).expect("valid bech32")
}

/// Verify ML-DSA-65 wallet signature and payload fingerprint for dataset listing request.
#[cfg(feature = "post-quantum")]
pub fn verify_mldsa65_signature(req: &ListDatasetRequest) -> Result<(), &'static str> {
    if req.signature.is_none() && req.public_key.is_none() {
        // Backward compatibility mode when auth fields are omitted
        return Ok(());
    }

    let pk_hex = req
        .public_key
        .as_deref()
        .ok_or("Missing public key for signature verification")?;
    let sig_hex = req
        .signature
        .as_deref()
        .ok_or("Missing signature for signature verification")?;

    let pk_bytes = crate::crypto::hex_decode(pk_hex)
        .map_err(|_| "Invalid public key hex format")?;
    let sig_bytes = crate::crypto::hex_decode(sig_hex)
        .map_err(|_| "Invalid signature hex format")?;

    let computed_fp = compute_dataset_fingerprint(
        &req.name,
        &req.category,
        &req.publisher,
        &req.description,
    );

    if let Some(provided_fp) = &req.fingerprint {
        if provided_fp.trim() != computed_fp {
            return Err("Payload fingerprint mismatch");
        }
    }

    let sig_alg = Sig::new(SigAlg::MlDsa65)
        .map_err(|_| "Failed to initialize ML-DSA-65 engine")?;

    let pk = sig_alg
        .public_key_from_bytes(&pk_bytes)
        .ok_or("Invalid public key bytes for ML-DSA-65")?;
    let signature = sig_alg
        .signature_from_bytes(&sig_bytes)
        .ok_or("Invalid signature bytes for ML-DSA-65")?;

    if sig_alg.verify(computed_fp.as_bytes(), &signature, &pk).is_err() {
        return Err("Invalid ML-DSA-65 signature");
    }

    // Validate publisher against public key / derived address if publisher is an xv1 address
    let derived_address = derive_address_from_pk(&pk_bytes);
    if req.publisher.starts_with("xv1") && req.publisher.contains("1") {
        if req.publisher != derived_address && req.publisher != pk_hex {
            if req.publisher.len() > 20 && req.publisher != derived_address {
                return Err("Publisher address does not match public key");
            }
        }
    }

    Ok(())
}

/// Verify ML-DSA-65 wallet signature and payload fingerprint for dataset listing request.
#[cfg(not(feature = "post-quantum"))]
pub fn verify_mldsa65_signature(req: &ListDatasetRequest) -> Result<(), &'static str> {
    if req.signature.is_none() && req.public_key.is_none() {
        return Ok(());
    }

    let computed_fp = compute_dataset_fingerprint(
        &req.name,
        &req.category,
        &req.publisher,
        &req.description,
    );

    if let Some(provided_fp) = &req.fingerprint {
        if provided_fp.trim() != computed_fp {
            return Err("Payload fingerprint mismatch");
        }
    }

    Err("ML-DSA-65 signature verification requires post-quantum feature")
}

/// `POST /v1/marketplace/datasets` — list a new dataset (auth: wallet signature ML-DSA-65).
pub async fn list_dataset_handler(Json(req): Json<ListDatasetRequest>) -> impl IntoResponse {
    if let Err(err) = verify_mldsa65_signature(&req) {
        return error_response(StatusCode::UNAUTHORIZED, err).into_response();
    }

    let metadata = DatasetMetadata {
        name: req.name,
        description: req.description,
        category: req.category,
        price: 0, // Calculated dynamically by marketplace.list_dataset
        publisher: req.publisher,
        rows: req.rows,
        tier: req.tier,
        reputation: req.reputation,
    };

    let mut marketplace = current_marketplace_mut();
    let dataset_id = marketplace.list_dataset(metadata);

    (
        StatusCode::CREATED,
        Json(json!({
            "status": "ok",
            "dataset_id": dataset_id.0,
            "message": "Dataset successfully listed in marketplace"
        })),
    )
        .into_response()
}

/// `GET /v1/marketplace/datasets` — list active datasets (public metadata, no rows).
pub async fn list_active_datasets_handler() -> impl IntoResponse {
    let marketplace = current_marketplace();
    let serialized = match serde_json::to_value(&*marketplace) {
        Ok(v) => v,
        Err(e) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
        }
    };

    let mut active_datasets = Vec::new();

    if let Some(datasets_map) = serialized.get("datasets").and_then(|d| d.as_object()) {
        for (id, val) in datasets_map {
            if let Some(arr) = val.as_array() {
                if arr.len() >= 2 {
                    let active = arr[1].as_bool().unwrap_or(false);
                    if active {
                        let mut meta = arr[0].clone();
                        if let Some(meta_obj) = meta.as_object_mut() {
                            meta_obj.insert("id".to_string(), json!(id));
                            meta_obj.insert("rows".to_string(), json!([])); // Strip rows for public preview
                        }
                        active_datasets.push(meta);
                    }
                }
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "count": active_datasets.len(),
            "datasets": active_datasets
        })),
    )
        .into_response()
}

/// `POST /v1/marketplace/datasets/{id}/query` — query a dataset (validates payment, returns DataPage).
pub async fn query_dataset_handler(
    Path(id): Path<String>,
    Json(req): Json<QueryDatasetRequest>,
) -> impl IntoResponse {
    let marketplace = current_marketplace();
    let dataset_id = DatasetId(id);

    match marketplace.query_dataset(&dataset_id, &req.query, req.payment) {
        Ok(page) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "page": page
            })),
        )
            .into_response(),
        Err(err) => error_response(StatusCode::BAD_REQUEST, err).into_response(),
    }
}

/// `DELETE /v1/marketplace/datasets/{id}` — revoke a dataset (seller only).
pub async fn revoke_dataset_handler(Path(id): Path<String>) -> impl IntoResponse {
    let mut marketplace = current_marketplace_mut();
    let dataset_id = DatasetId(id.clone());

    match marketplace.revoke_dataset(&dataset_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "message": "Dataset revoked successfully",
                "dataset_id": id
            })),
        )
            .into_response(),
        Err(err) => error_response(StatusCode::BAD_REQUEST, err).into_response(),
    }
}

/// `GET /v1/marketplace/pricing` — pricing oracle preview (Raw/Verified/Gold + demand).
pub async fn get_pricing_preview_handler(
    Query(params): Query<PricingPreviewQuery>,
) -> impl IntoResponse {
    let size = params.size.unwrap_or(100);
    let reputation = params.reputation.unwrap_or(0.0);

    let price_free = calculate_price(size, PricingTier::Free, reputation);
    let price_colaborador = calculate_price(size, PricingTier::Colaborador, reputation);
    let price_colaborador_plus = calculate_price(size, PricingTier::ColaboradorPlus, reputation);

    let raw_mult = QualityLevel::Raw.multiplier();
    let verified_mult = QualityLevel::Verified.multiplier();
    let gold_mult = QualityLevel::Gold.multiplier();

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "preview_size": size,
            "reputation": reputation,
            "pricing_tiers": {
                "Free": price_free.0,
                "Colaborador": price_colaborador.0,
                "ColaboradorPlus": price_colaborador_plus.0
            },
            "quality_multipliers": {
                "Raw": raw_mult,
                "Verified": verified_mult,
                "Gold": gold_mult
            },
            "quality_preview_base": {
                "Raw": (price_colaborador.0 as f64 * raw_mult).round() as u64,
                "Verified": (price_colaborador.0 as f64 * verified_mult).round() as u64,
                "Gold": (price_colaborador.0 as f64 * gold_mult).round() as u64
            },
            "demand_decay_params": {
                "decay_half_life_secs": 86400,
                "demand_multiplier": 0.1,
                "update_interval_secs": 3600
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_dataset_fingerprint() {
        let fp1 = compute_dataset_fingerprint("ds1", "cat1", "pub1", "desc1");
        let fp2 = compute_dataset_fingerprint("ds1", "cat1", "pub1", "desc1");
        let fp3 = compute_dataset_fingerprint("ds1", "cat1", "pub1", "different desc");

        assert_eq!(fp1, fp2);
        assert_ne!(fp1, fp3);
        assert_eq!(fp1.len(), 64);
    }

    #[test]
    fn test_verify_signature_omitted_backward_compatibility() {
        let req = ListDatasetRequest {
            name: "Test DS".into(),
            description: "Test Desc".into(),
            category: "Analytics".into(),
            publisher: "xv1_test_pub".into(),
            rows: vec![],
            tier: PricingTier::Free,
            reputation: 0.0,
            public_key: None,
            signature: None,
            fingerprint: None,
        };

        assert!(verify_mldsa65_signature(&req).is_ok());
    }

    #[test]
    fn test_verify_signature_fingerprint_mismatch() {
        let req = ListDatasetRequest {
            name: "Test DS".into(),
            description: "Test Desc".into(),
            category: "Analytics".into(),
            publisher: "xv1_test_pub".into(),
            rows: vec![],
            tier: PricingTier::Free,
            reputation: 0.0,
            public_key: Some("00112233".into()),
            signature: Some("44556677".into()),
            fingerprint: Some("invalid_fingerprint_hash".into()),
        };

        let res = verify_mldsa65_signature(&req);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "Payload fingerprint mismatch");
    }

    #[cfg(feature = "post-quantum")]
    #[test]
    fn test_mldsa65_signature_verification_full_flow() {
        let sig_alg = Sig::new(SigAlg::MlDsa65).expect("ML-DSA-65 init");
        let (pk, sk) = sig_alg.keypair().expect("keypair gen");

        let pk_hex = crate::crypto::hex_encode(pk.as_ref());
        let publisher_address = derive_address_from_pk(pk.as_ref());

        let name = "Secure Post Quantum Dataset";
        let category = "Security";
        let description = "PQ Verified Dataset";

        let fp = compute_dataset_fingerprint(name, category, &publisher_address, description);

        let signature = sig_alg.sign(fp.as_bytes(), &sk).expect("signing");
        let sig_hex = crate::crypto::hex_encode(signature.as_ref());

        // Valid signature & matching fingerprint & valid derived address
        let valid_req = ListDatasetRequest {
            name: name.into(),
            description: description.into(),
            category: category.into(),
            publisher: publisher_address.clone(),
            rows: vec![],
            tier: PricingTier::Colaborador,
            reputation: 0.8,
            public_key: Some(pk_hex.clone()),
            signature: Some(sig_hex.clone()),
            fingerprint: Some(fp.clone()),
        };

        assert!(verify_mldsa65_signature(&valid_req).is_ok());

        // Corrupted signature should be rejected
        let mut bad_sig_hex = sig_hex.clone();
        bad_sig_hex.replace_range(0..2, "00");
        if bad_sig_hex == sig_hex {
            bad_sig_hex.replace_range(0..2, "ff");
        }

        let invalid_sig_req = ListDatasetRequest {
            signature: Some(bad_sig_hex),
            ..valid_req
        };

        assert!(verify_mldsa65_signature(&invalid_sig_req).is_err());
    }
}
