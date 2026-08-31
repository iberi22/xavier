//! HTTP handler for training dataset operations
//!
//! Provides API endpoints for listing dataset manifests, retrieving manifests and splits (JSONL),
//! and generating new training dataset bundles.

use crate::data_commons::training::{
    load_dataset_manifest, load_dataset_metadata, load_dataset_split, scan_datasets,
    write_bundle_to_dir, TrainingExporter,
};
use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn get_data_dir() -> PathBuf {
    std::env::var("XAVIER_TRAINING_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/datasets"))
}

fn get_telemetry_db_path() -> PathBuf {
    std::env::var("XAVIER_TELEMETRY_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".xavier/telemetry.db"))
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GenerateBundlePayload {
    pub seed: u64,
    pub eval_ratio: f32,
    pub clearance: Option<String>,
    pub language: Option<String>,
    pub segment: Option<String>,
}

/// Handler for `GET /v1/training/datasets`
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

/// Handler for `GET /v1/training/datasets/{id}`
pub async fn get_dataset_manifest_handler(Path(id): Path<String>) -> impl IntoResponse {
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
    let dataset_dir = get_data_dir().join(&id);
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

/// Handler for `GET /v1/training/datasets/{id}/train`
pub async fn get_dataset_train_handler(Path(id): Path<String>) -> impl IntoResponse {
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
    let dataset_dir = get_data_dir().join(&id);
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

/// Handler for `GET /v1/training/datasets/{id}/eval`
pub async fn get_dataset_eval_handler(Path(id): Path<String>) -> impl IntoResponse {
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
    let dataset_dir = get_data_dir().join(&id);
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

/// Handler for `POST /v1/training/bundles`
pub async fn generate_bundle_handler(
    Json(payload): Json<GenerateBundlePayload>,
) -> impl IntoResponse {
    let db_path = get_telemetry_db_path();
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
                        "message": format!("Failed to write bundle: {}", e),
                    })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Failed to generate bundle: {}", e),
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::training::{write_bundle_to_dir, AuditSummary, TrainingBundle};
    use tempfile::tempdir;

    #[test]
    fn test_training_handler_data_dir_env() {
        std::env::set_var("XAVIER_TRAINING_DATA_DIR", "/tmp/test_datasets_dir");
        assert_eq!(get_data_dir(), PathBuf::from("/tmp/test_datasets_dir"));
        std::env::remove_var("XAVIER_TRAINING_DATA_DIR");
    }

    #[test]
    fn test_training_bundle_write_and_load_split() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path();

        let bundle = TrainingBundle {
            manifest: crate::data_commons::readiness::TrainingBundleManifest {
                version: "1.0.0".to_string(),
                usage_policy: "Testing".to_string(),
                reproducibility_seed: 7,
                split_counts: [("train".to_string(), 2), ("eval".to_string(), 1)]
                    .into_iter()
                    .collect(),
                data_files: vec!["train.jsonl".to_string(), "eval.jsonl".to_string()],
            },
            train_split: vec![serde_json::json!({"t": 1}), serde_json::json!({"t": 2})],
            eval_split: vec![serde_json::json!({"e": 1})],
            audit_summary: AuditSummary {
                total_records_found: 3,
                included_records: 3,
                excluded_records_no_consent: 0,
                excluded_records_revoked: 0,
            },
        };

        write_bundle_to_dir(
            data_dir,
            "ds_test_01",
            &bundle,
            Some("PUBLIC".to_string()),
            Some("en".to_string()),
            Some("test_segment".to_string()),
        )
        .unwrap();

        let datasets = scan_datasets(data_dir).unwrap();
        assert_eq!(datasets.len(), 1);
        assert_eq!(datasets[0].id, "ds_test_01");
        assert_eq!(datasets[0].clearance, "PUBLIC");

        let train_str = load_dataset_split(&data_dir.join("ds_test_01"), "train").unwrap();
        assert_eq!(train_str.lines().count(), 2);

        let eval_str = load_dataset_split(&data_dir.join("ds_test_01"), "eval").unwrap();
        assert_eq!(eval_str.lines().count(), 1);
    }
}
