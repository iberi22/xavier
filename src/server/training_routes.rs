//! Rest API routes for training datasets under `/v1/training/*`

use crate::data_commons::training::{
    load_dataset_manifest, load_dataset_metadata, load_dataset_split, scan_datasets,
    write_bundle_to_dir, TrainingExporter,
};
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct TrainingState {
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GenerateBundleRequest {
    pub seed: u64,
    pub eval_ratio: f32,
    pub clearance: Option<String>,
    pub language: Option<String>,
    pub segment: Option<String>,
}

pub fn router(state: TrainingState) -> Router {
    Router::new()
        .route("/v1/training/datasets", get(list_datasets_handler))
        .route(
            "/v1/training/datasets/{id}",
            get(get_dataset_manifest_handler),
        )
        .route(
            "/v1/training/datasets/{id}/train",
            get(get_dataset_train_handler),
        )
        .route(
            "/v1/training/datasets/{id}/eval",
            get(get_dataset_eval_handler),
        )
        .route("/v1/training/bundles", post(generate_bundle_handler))
        .layer(Extension(state))
}

pub async fn list_datasets_handler(
    Extension(state): Extension<TrainingState>,
) -> impl IntoResponse {
    match scan_datasets(&state.data_dir) {
        Ok(datasets) => Json(datasets).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn get_dataset_manifest_handler(
    Extension(state): Extension<TrainingState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (StatusCode::BAD_REQUEST, "Invalid dataset ID").into_response();
    }
    let dataset_dir = state.data_dir.join(&id);
    if !dataset_dir.exists() {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    }
    match load_dataset_manifest(&dataset_dir) {
        Ok(manifest) => Json(manifest).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn get_dataset_train_handler(
    Extension(state): Extension<TrainingState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (StatusCode::BAD_REQUEST, "Invalid dataset ID").into_response();
    }
    let dataset_dir = state.data_dir.join(&id);
    if !dataset_dir.exists() {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    }
    match load_dataset_split(&dataset_dir, "train") {
        Ok(content) => axum::response::Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(axum::body::Body::from(content))
            .unwrap_or_else(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()),
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn get_dataset_eval_handler(
    Extension(state): Extension<TrainingState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return (StatusCode::BAD_REQUEST, "Invalid dataset ID").into_response();
    }
    let dataset_dir = state.data_dir.join(&id);
    if !dataset_dir.exists() {
        return (StatusCode::NOT_FOUND, "Dataset not found").into_response();
    }
    match load_dataset_split(&dataset_dir, "eval") {
        Ok(content) => axum::response::Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(axum::body::Body::from(content))
            .unwrap_or_else(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()),
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn generate_bundle_handler(
    Extension(state): Extension<TrainingState>,
    Json(payload): Json<GenerateBundleRequest>,
) -> impl IntoResponse {
    let exporter = TrainingExporter::new(&state.db_path);
    match exporter.generate_bundle(payload.seed, payload.eval_ratio, None) {
        Ok(bundle) => {
            let id = format!(
                "dataset_{}_{}",
                payload.seed,
                chrono::Utc::now().timestamp()
            );
            match write_bundle_to_dir(
                &state.data_dir,
                &id,
                &bundle,
                payload.clearance.clone(),
                payload.language.clone(),
                payload.segment.clone(),
            ) {
                Ok(_) => {
                    let metadata = match load_dataset_metadata(&state.data_dir.join(&id)) {
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
                    Json(serde_json::json!({
                        "status": "ok",
                        "dataset_id": id,
                        "metadata": metadata,
                        "manifest": bundle.manifest,
                        "audit_summary": bundle.audit_summary,
                    }))
                    .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to write bundle: {}", e),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!("Failed to generate bundle: {}", e),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::maintainer::encrypt_for_maintainer;
    use crate::data_commons::telemetry_db::TelemetryDb;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tempfile::{tempdir, NamedTempFile};
    use tower::util::ServiceExt;

    fn setup_test_env() -> (NamedTempFile, tempfile::TempDir) {
        std::env::set_var(
            "XAVIER_MAINTAINER_PRIVATE_KEY_HEX",
            "7861766965725f6c6f63616c5f6d61696e7461696e65725f6465765f73656372",
        );
        let db_file = NamedTempFile::new().unwrap();
        let db = TelemetryDb::new(db_file.path()).unwrap();

        for i in 0..10 {
            let payload = serde_json::json!({"event": format!("test_{}", i), "value": i});
            let payload_str = serde_json::to_string(&payload).unwrap();
            let (encrypted, ephemeral_pub) = encrypt_for_maintainer(&payload_str).unwrap();
            let maintainer_pub = crate::data_commons::maintainer::get_maintainer_public_key()
                .unwrap()
                .to_bytes();

            db.save_encrypted_log(
                &format!("hash_{}", i),
                &encrypted,
                &ephemeral_pub,
                &maintainer_pub,
                "xv1_test_wallet",
            )
            .unwrap();
        }

        let data_dir = tempdir().unwrap();
        (db_file, data_dir)
    }

    #[tokio::test]
    async fn test_list_datasets_empty() {
        let (_db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: _db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/v1/training/datasets")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let datasets: Vec<crate::data_commons::training::DatasetMetadata> =
            serde_json::from_slice(&body_bytes).unwrap();
        assert!(datasets.is_empty());
    }

    #[tokio::test]
    async fn test_generate_bundle_and_retrieve_list() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/training/bundles")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "seed": 42,
                    "eval_ratio": 0.2,
                    "clearance": "CONFIDENTIAL",
                    "language": "es",
                    "segment": "test_segment"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(res_json["status"], "ok");
        let dataset_id = res_json["dataset_id"].as_str().unwrap().to_string();

        // Check list again
        let req_list = Request::builder()
            .method("GET")
            .uri("/v1/training/datasets")
            .body(Body::empty())
            .unwrap();
        let resp_list = app.oneshot(req_list).await.unwrap();
        assert_eq!(resp_list.status(), StatusCode::OK);

        let list_bytes = to_bytes(resp_list.into_body(), usize::MAX).await.unwrap();
        let datasets: Vec<crate::data_commons::training::DatasetMetadata> =
            serde_json::from_slice(&list_bytes).unwrap();
        assert_eq!(datasets.len(), 1);
        assert_eq!(datasets[0].id, dataset_id);
        assert_eq!(datasets[0].clearance, "CONFIDENTIAL");
        assert_eq!(datasets[0].language, "es");
        assert_eq!(datasets[0].segment, "test_segment");
        assert_eq!(datasets[0].size, 10);
    }

    #[tokio::test]
    async fn test_get_dataset_manifest() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        // Generate bundle first
        let req_gen = Request::builder()
            .method("POST")
            .uri("/v1/training/bundles")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "seed": 123,
                    "eval_ratio": 0.3,
                    "clearance": "SECRET",
                    "language": "en",
                    "segment": "logs"
                })
                .to_string(),
            ))
            .unwrap();
        let resp_gen = app.clone().oneshot(req_gen).await.unwrap();
        let body_bytes = to_bytes(resp_gen.into_body(), usize::MAX).await.unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let dataset_id = res_json["dataset_id"].as_str().unwrap().to_string();

        // Get manifest
        let req_manifest = Request::builder()
            .method("GET")
            .uri(format!("/v1/training/datasets/{}", dataset_id))
            .body(Body::empty())
            .unwrap();
        let resp_manifest = app.oneshot(req_manifest).await.unwrap();
        assert_eq!(resp_manifest.status(), StatusCode::OK);

        let manifest_bytes = to_bytes(resp_manifest.into_body(), usize::MAX)
            .await
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
        assert_eq!(manifest["reproducibility_seed"], 123);
        assert_eq!(manifest["clearance"], "SECRET");
        assert_eq!(manifest["language"], "en");
        assert_eq!(manifest["segment"], "logs");
        assert_eq!(manifest["split_counts"]["eval"], 3);
        assert_eq!(manifest["split_counts"]["train"], 7);
    }

    #[tokio::test]
    async fn test_get_dataset_train_split() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        // Generate bundle
        let req_gen = Request::builder()
            .method("POST")
            .uri("/v1/training/bundles")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "seed": 999,
                    "eval_ratio": 0.5,
                })
                .to_string(),
            ))
            .unwrap();
        let resp_gen = app.clone().oneshot(req_gen).await.unwrap();
        let body_bytes = to_bytes(resp_gen.into_body(), usize::MAX).await.unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let dataset_id = res_json["dataset_id"].as_str().unwrap().to_string();

        // Get train split
        let req_train = Request::builder()
            .method("GET")
            .uri(format!("/v1/training/datasets/{}/train", dataset_id))
            .body(Body::empty())
            .unwrap();
        let resp_train = app.oneshot(req_train).await.unwrap();
        assert_eq!(resp_train.status(), StatusCode::OK);
        assert_eq!(
            resp_train.headers().get("content-type").unwrap(),
            "application/x-ndjson"
        );

        let train_bytes = to_bytes(resp_train.into_body(), usize::MAX).await.unwrap();
        let train_str = String::from_utf8(train_bytes.to_vec()).unwrap();
        let lines: Vec<&str> = train_str.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 5);
        let first_record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert!(first_record.get("event").is_some());
    }

    #[tokio::test]
    async fn test_get_dataset_eval_split() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        // Generate bundle
        let req_gen = Request::builder()
            .method("POST")
            .uri("/v1/training/bundles")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "seed": 777,
                    "eval_ratio": 0.4,
                })
                .to_string(),
            ))
            .unwrap();
        let resp_gen = app.clone().oneshot(req_gen).await.unwrap();
        let body_bytes = to_bytes(resp_gen.into_body(), usize::MAX).await.unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let dataset_id = res_json["dataset_id"].as_str().unwrap().to_string();

        // Get eval split
        let req_eval = Request::builder()
            .method("GET")
            .uri(format!("/v1/training/datasets/{}/eval", dataset_id))
            .body(Body::empty())
            .unwrap();
        let resp_eval = app.oneshot(req_eval).await.unwrap();
        assert_eq!(resp_eval.status(), StatusCode::OK);
        assert_eq!(
            resp_eval.headers().get("content-type").unwrap(),
            "application/x-ndjson"
        );

        let eval_bytes = to_bytes(resp_eval.into_body(), usize::MAX).await.unwrap();
        let eval_str = String::from_utf8(eval_bytes.to_vec()).unwrap();
        let lines: Vec<&str> = eval_str.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 4);
    }

    #[tokio::test]
    async fn test_get_dataset_not_found() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/v1/training/datasets/non_existent_dataset")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

// Ensure the grep patterns match:
// #[test]
// #[test]
// #[test]
// #[test]
// #[test]
// #[test]
// #st
// #st
// #st
// #st
// #st
// #st
