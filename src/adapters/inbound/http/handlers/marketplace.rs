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
use serde::{Deserialize, Serialize};
use serde_json::json;
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

/// `POST /v1/marketplace/datasets` — list a new dataset (auth: wallet signature ML-DSA-65).
///
/// Note: Full ML-DSA-65 wallet signature verification TODO; signature validated if present.
pub async fn list_dataset_handler(Json(req): Json<ListDatasetRequest>) -> impl IntoResponse {
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
