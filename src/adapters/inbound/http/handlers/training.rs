//! Training Datasets HTTP Handlers
//!
//! Provides axum HTTP handlers for training dataset listing, manifests, JSONL splits,
//! and bundle creation using `TrainingExporter`.

use crate::adapters::inbound::http::state::check_auth;
use crate::adapters::inbound::http::AppState;
use crate::data_commons::training::{
    load_dataset_manifest, load_dataset_metadata, load_dataset_split, scan_datasets,
    write_bundle_to_dir, TrainingExporter,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CreateBundlePayload {
    pub seed: u64,
    pub eval_ratio: f32,
    pub clearance: Option<String>,
    pub language: Option<String>,
    pub segment: Option<String>,
}

fn get_data_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("XAVIER_DATASETS_DIR")
            .unwrap_or_else(|_| "data/training/datasets".to_string()),
    )
}

fn get_db_path() -> PathBuf {
    PathBuf::from(
        std::env::var("XAVIER_TELEMETRY_DB_PATH")
            .unwrap_or_else(|_| ".xavier/telemetry.db".to_string()),
    )
}

/// GET /v1/training/datasets — list all training datasets.
pub async fn list_datasets_handler() -> impl IntoResponse {
    let data_dir = get_data_dir();
    match scan_datasets(&data_dir) {
        Ok(datasets) => (StatusCode::OK, Json(datasets)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "status": "error", "message": e })),
        )
            .into_response(),
    }
}

/// GET /v1/training/datasets/{id} — get manifest for a dataset.
pub async fn get_manifest_handler(Path(id): Path<String>) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": "Invalid dataset ID" })),
        )
            .into_response();
    }
    let data_dir = get_data_dir();
    let dataset_dir = data_dir.join(&id);
    if !dataset_dir.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "error", "message": "Dataset not found" })),
        )
            .into_response();
    }
    match load_dataset_manifest(&dataset_dir) {
        Ok(manifest) => (StatusCode::OK, Json(manifest)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "status": "error", "message": e })),
        )
            .into_response(),
    }
}

/// GET /v1/training/datasets/{id}/train — get train JSONL split.
pub async fn get_train_split_handler(Path(id): Path<String>) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": "Invalid dataset ID" })),
        )
            .into_response();
    }
    let data_dir = get_data_dir();
    let dataset_dir = data_dir.join(&id);
    if !dataset_dir.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "error", "message": "Dataset not found" })),
        )
            .into_response();
    }
    match load_dataset_split(&dataset_dir, "train") {
        Ok(content) => axum::response::Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(axum::body::Body::from(content))
            .unwrap_or_else(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
                )
                    .into_response()
            }),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "error", "message": e })),
        )
            .into_response(),
    }
}

/// GET /v1/training/datasets/{id}/eval — get eval JSONL split.
pub async fn get_eval_split_handler(Path(id): Path<String>) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "message": "Invalid dataset ID" })),
        )
            .into_response();
    }
    let data_dir = get_data_dir();
    let dataset_dir = data_dir.join(&id);
    if !dataset_dir.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "error", "message": "Dataset not found" })),
        )
            .into_response();
    }
    match load_dataset_split(&dataset_dir, "eval") {
        Ok(content) => axum::response::Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(axum::body::Body::from(content))
            .unwrap_or_else(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
                )
                    .into_response()
            }),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "status": "error", "message": e })),
        )
            .into_response(),
    }
}

/// POST /v1/training/bundles — generate a new dataset bundle.
pub async fn create_bundle_handler(Json(payload): Json<CreateBundlePayload>) -> impl IntoResponse {
    let db_path = get_db_path();
    let data_dir = get_data_dir();
    let exporter = TrainingExporter::new(&db_path);

    match exporter.generate_bundle(payload.seed, payload.eval_ratio, None) {
        Ok(bundle) => {
            let id = format!(
                "dataset_{}_{}",
                payload.seed,
                chrono::Utc::now().timestamp()
            );
            match write_bundle_to_dir(
                &data_dir,
                &id,
                &bundle,
                payload.clearance.clone(),
                payload.language.clone(),
                payload.segment.clone(),
            ) {
                Ok(_) => {
                    let metadata = match load_dataset_metadata(&data_dir.join(&id)) {
                        Ok(meta) => meta,
                        Err(_) => crate::data_commons::training::DatasetMetadata {
                            id: id.clone(),
                            size: bundle.train_split.len() + bundle.eval_split.len(),
                            clearance: payload
                                .clearance
                                .clone()
                                .unwrap_or_else(|| "INTERNAL".to_string()),
                            language: payload.language.clone().unwrap_or_else(|| "en".to_string()),
                            segment: payload
                                .segment
                                .clone()
                                .unwrap_or_else(|| "telemetry".to_string()),
                        },
                    };
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": "ok",
                            "dataset_id": id,
                            "metadata": metadata,
                            "manifest": bundle.manifest,
                            "audit_summary": bundle.audit_summary,
                        })),
                    )
                        .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Failed to write bundle: {}", e)
                    })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Failed to generate bundle: {}", e)
            })),
        )
            .into_response(),
    }
}
