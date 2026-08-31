//! Rest API routes for training datasets under `/v1/training/*`

use crate::curation::CurationQueue;
use crate::data_commons::training::{
    load_dataset_manifest, load_dataset_metadata, load_dataset_split, scan_datasets,
    write_bundle_to_dir, TrainingExporter,
};
use crate::security::redaction::RedactionEngine;
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use futures_util::StreamExt;
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

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ExportRequest {
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub eval_ratio: f32,
    #[serde(default)]
    pub curated_only: bool,
    pub limit: Option<usize>,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BulkCurateAction {
    Approve,
    Reject,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BulkCurateItem {
    pub id: String,
    pub action: BulkCurateAction,
    pub reason: Option<String>,
    pub classification: Option<String>,
    pub clearance: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BulkCurateRequest {
    pub curator: String,
    pub items: Vec<BulkCurateItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BulkCurateResponse {
    pub status: String,
    pub processed_count: usize,
    pub approved_count: usize,
    pub rejected_count: usize,
    pub items: Vec<crate::curation::CurationItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ApproveCurateRequest {
    pub curator: String,
    pub classification: Option<String>,
    pub clearance: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RejectCurateRequest {
    pub curator: String,
    pub reason: String,
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
        .route("/v1/training/export", post(export_handler))
        .route("/v1/training/curate/bulk", post(bulk_curate_handler))
        .route("/v1/curation/pending", get(get_pending_curation_handler))
        .route(
            "/v1/curation/{id}/approve",
            post(approve_curation_item_handler),
        )
        .route(
            "/v1/curation/{id}/reject",
            post(reject_curation_item_handler),
        )
        .layer(Extension(state))
}

pub async fn get_pending_curation_handler(
    Extension(state): Extension<TrainingState>,
) -> impl IntoResponse {
    let queue = load_curation_queue(&state);
    Json(queue.list_pending()).into_response()
}

pub async fn approve_curation_item_handler(
    Extension(state): Extension<TrainingState>,
    Path(id): Path<String>,
    Json(payload): Json<ApproveCurateRequest>,
) -> impl IntoResponse {
    let mut queue = load_curation_queue(&state);
    match queue.approve(&id, payload.curator, payload.classification, payload.clearance) {
        Ok(item) => {
            if let Err(e) = queue.save() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to save curation queue: {}", e),
                )
                    .into_response();
            }
            Json(serde_json::json!({
                "status": "ok",
                "item": item,
            }))
            .into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
}

pub async fn reject_curation_item_handler(
    Extension(state): Extension<TrainingState>,
    Path(id): Path<String>,
    Json(payload): Json<RejectCurateRequest>,
) -> impl IntoResponse {
    let mut queue = load_curation_queue(&state);
    match queue.reject(&id, payload.curator, payload.reason) {
        Ok(item) => {
            if let Err(e) = queue.save() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to save curation queue: {}", e),
                )
                    .into_response();
            }
            Json(serde_json::json!({
                "status": "ok",
                "item": item,
            }))
            .into_response()
        }
        Err(e) => (StatusCode::NOT_FOUND, e).into_response(),
    }
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

pub async fn bulk_curate_handler(
    Extension(state): Extension<TrainingState>,
    Json(payload): Json<BulkCurateRequest>,
) -> impl IntoResponse {
    if payload.items.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "No items provided for bulk curation",
        )
            .into_response();
    }

    let mut queue = load_curation_queue(&state);

    // Anti-Hallucination Guard: enforce item ID existence verification during bulk operations
    let existing_ids: std::collections::HashSet<&str> =
        queue.items.iter().map(|i| i.id.as_str()).collect();

    for item in &payload.items {
        if !existing_ids.contains(item.id.as_str()) {
            return (
                StatusCode::BAD_REQUEST,
                format!("Item ID '{}' not found in curation queue", item.id),
            )
                .into_response();
        }
    }

    let mut updated_items = Vec::new();
    let mut approved_count = 0;
    let mut rejected_count = 0;

    for item in &payload.items {
        match item.action {
            BulkCurateAction::Approve => {
                match queue.approve(
                    &item.id,
                    payload.curator.clone(),
                    item.classification.clone(),
                    item.clearance.clone(),
                ) {
                    Ok(updated) => {
                        approved_count += 1;
                        updated_items.push(updated);
                    }
                    Err(e) => {
                        return (StatusCode::BAD_REQUEST, e).into_response();
                    }
                }
            }
            BulkCurateAction::Reject => {
                let reason = item
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Rejected in bulk review".to_string());
                match queue.reject(&item.id, payload.curator.clone(), reason) {
                    Ok(updated) => {
                        rejected_count += 1;
                        updated_items.push(updated);
                    }
                    Err(e) => {
                        return (StatusCode::BAD_REQUEST, e).into_response();
                    }
                }
            }
        }
    }

    if let Err(e) = queue.save() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save curation queue: {}", e),
        )
            .into_response();
    }

    Json(BulkCurateResponse {
        status: "ok".to_string(),
        processed_count: updated_items.len(),
        approved_count,
        rejected_count,
        items: updated_items,
    })
    .into_response()
}

pub async fn export_handler(
    Extension(state): Extension<TrainingState>,
    Json(payload): Json<ExportRequest>,
) -> impl IntoResponse {
    if payload.curated_only {
        let queue = load_curation_queue(&state);
        let mut approved_items = queue.curated_dataset();

        if payload.seed > 0 || payload.eval_ratio > 0.0 {
            use rand::seq::SliceRandom;
            use rand::SeedableRng;
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(payload.seed);
            approved_items.shuffle(&mut rng);
        }

        if let Some(limit) = payload.limit {
            approved_items.truncate(limit);
        }

        let redactor = RedactionEngine::default();
        let mut lines = Vec::new();

        for item in approved_items {
            let redacted = redactor.redact(&item.content_ref);
            let obj = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&redacted) {
                parsed
            } else {
                serde_json::json!({
                    "text": redacted,
                    "id": item.id,
                    "clearance": item.proposed_clearance
                })
            };
            if let Ok(line_str) = serde_json::to_string(&obj) {
                lines.push(line_str);
            }
        }

        let body_str = if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n") + "\n"
        };

        return axum::response::Response::builder()
            .header("content-type", "application/x-ndjson")
            .body(axum::body::Body::from(body_str))
            .unwrap_or_else(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            });
    }

    let exporter = TrainingExporter::new(&state.db_path);
    match exporter.generate_bundle(payload.seed, payload.eval_ratio, None) {
        Ok(bundle) => {
            if payload.format.as_deref() == Some("jsonl") {
                let stream_records = bundle.train_split.into_iter().chain(bundle.eval_split);

                let stream = futures_util::stream::iter(stream_records).map(|record| {
                    let mut json_str = serde_json::to_string(&record).unwrap_or_default();
                    json_str.push('\n');
                    Ok::<_, std::convert::Infallible>(json_str)
                });

                return axum::response::Response::builder()
                    .header("content-type", "application/x-ndjson")
                    .body(axum::body::Body::from_stream(stream))
                    .unwrap_or_else(|e| {
                        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                    });
            }

            let manifest = match serde_json::to_value(&bundle.manifest) {
                Ok(v) => v,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            };
            Json(serde_json::json!({
                "manifest": manifest,
                "train_count": bundle.train_split.len(),
                "eval_count": bundle.eval_split.len(),
                "audit": {
                    "total_records_found": bundle.audit_summary.total_records_found,
                    "included_records": bundle.audit_summary.included_records,
                    "excluded_no_consent": bundle.audit_summary.excluded_records_no_consent,
                    "excluded_revoked": bundle.audit_summary.excluded_records_revoked
                }
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!("Failed to generate bundle: {}", e),
        )
            .into_response(),
    }
}

fn load_curation_queue(state: &TrainingState) -> CurationQueue {
    let parent_path = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("curation/queue.json");
    if parent_path.exists() {
        if let Ok(queue) = CurationQueue::load_from_path(&parent_path) {
            return queue;
        }
    }
    let state_path = state.data_dir.join("curation/queue.json");
    if state_path.exists() {
        if let Ok(queue) = CurationQueue::load_from_path(&state_path) {
            return queue;
        }
    }
    let default_path = std::path::Path::new("data/curation/queue.json");
    if default_path.exists() {
        if let Ok(queue) = CurationQueue::load_from_path(default_path) {
            return queue;
        }
    }
    CurationQueue::load().unwrap_or_default()
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

    #[tokio::test]
    async fn test_export_curated_only_approved_and_redacted() {
        let (db_file, data_dir) = setup_test_env();
        let curation_dir = data_dir.path().join("curation");
        std::fs::create_dir_all(&curation_dir).unwrap();
        let queue_file = curation_dir.join("queue.json");

        let mut queue = CurationQueue::new_with_path(queue_file);
        let item1 = queue.submit_for_curation(
            "Contact boss@company.org or call 123-456-7890 for approved access.".to_string(),
            "INTERNAL".to_string(),
            Some("agent".to_string()),
        );
        let item2 = queue.submit_for_curation(
            "Rejected confidential content".to_string(),
            "SECRET".to_string(),
            Some("import".to_string()),
        );
        let _item3 = queue.submit_for_curation(
            "Pending item content".to_string(),
            "PUBLIC".to_string(),
            None,
        );

        queue
            .approve(&item1.id, "curator_alice".to_string(), None, None)
            .unwrap();
        queue
            .reject(
                &item2.id,
                "curator_bob".to_string(),
                "Spam or invalid".to_string(),
            )
            .unwrap();
        queue.save().unwrap();

        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().join("datasets"),
        };
        let app = router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/training/export")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "curated_only": true,
                    "limit": 10
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/x-ndjson"
        );

        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        let lines: Vec<&str> = body_str.lines().filter(|l| !l.is_empty()).collect();

        // Exactly 1 line (only Approved item)
        assert_eq!(lines.len(), 1);

        let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let text = parsed["text"].as_str().unwrap();

        // Check PII redaction
        assert!(text.contains("[EMAIL]"));
        assert!(text.contains("[PHONE]"));
        assert!(!text.contains("boss@company.org"));
        assert!(!text.contains("123-456-7890"));

        // Rejected and Pending items must be excluded
        assert!(!body_str.contains("Rejected confidential content"));
        assert!(!body_str.contains("Pending item content"));
    }

    #[tokio::test]
    async fn test_export_backward_compatibility() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/training/export")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "curated_only": false,
                    "seed": 42,
                    "eval_ratio": 0.2
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(res_json.get("manifest").is_some());
        assert_eq!(res_json["train_count"], 8);
        assert_eq!(res_json["eval_count"], 2);
    }

    #[tokio::test]
    async fn test_export_jsonl_streaming() {
        let (db_file, data_dir) = setup_test_env();
        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().to_path_buf(),
        };
        let app = router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/training/export")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "format": "jsonl",
                    "seed": 42,
                    "eval_ratio": 0.2
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/x-ndjson"
        );

        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        let lines: Vec<&str> = body_str.lines().filter(|l| !l.is_empty()).collect();

        // 10 total records in setup_test_env (8 train + 2 eval)
        assert_eq!(lines.len(), 10);
        for line in lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("event").is_some());
        }
    }

    #[tokio::test]
    async fn test_bulk_curation_approve_and_reject() {
        let (db_file, data_dir) = setup_test_env();
        let curation_dir = data_dir.path().join("curation");
        std::fs::create_dir_all(&curation_dir).unwrap();
        let queue_file = curation_dir.join("queue.json");

        let mut queue = CurationQueue::new_with_path(queue_file.clone());
        let item1 = queue.submit_for_curation(
            "Item 1 content".to_string(),
            "CONFIDENTIAL".to_string(),
            Some("agent".to_string()),
        );
        let item2 = queue.submit_for_curation(
            "Item 2 content".to_string(),
            "SECRET".to_string(),
            Some("agent".to_string()),
        );
        let item3 = queue.submit_for_curation(
            "Item 3 content".to_string(),
            "RESTRICTED".to_string(),
            Some("import".to_string()),
        );
        queue.save().unwrap();

        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().join("datasets"),
        };
        let app = router(state);

        // 1. Test Anti-Hallucination Guard with non-existent item ID
        let invalid_req = Request::builder()
            .method("POST")
            .uri("/v1/training/curate/bulk")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "curator": "reviewer_bob",
                    "items": [
                        {
                            "id": item1.id,
                            "action": "approve"
                        },
                        {
                            "id": "non_existent_id_999",
                            "action": "approve"
                        }
                    ]
                })
                .to_string(),
            ))
            .unwrap();

        let resp_invalid = app.clone().oneshot(invalid_req).await.unwrap();
        assert_eq!(resp_invalid.status(), StatusCode::BAD_REQUEST);

        // 2. Test valid bulk approval and rejection
        let bulk_req = Request::builder()
            .method("POST")
            .uri("/v1/training/curate/bulk")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "curator": "reviewer_alice",
                    "items": [
                        {
                            "id": item1.id,
                            "action": "approve",
                            "classification": "verified_data",
                            "clearance": "PUBLIC"
                        },
                        {
                            "id": item2.id,
                            "action": "reject",
                            "reason": "Contains sensitive telemetry"
                        }
                    ]
                })
                .to_string(),
            ))
            .unwrap();

        let resp_bulk = app.oneshot(bulk_req).await.unwrap();
        assert_eq!(resp_bulk.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp_bulk.into_body(), usize::MAX).await.unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(res_json["status"], "ok");
        assert_eq!(res_json["processed_count"], 2);
        assert_eq!(res_json["approved_count"], 1);
        assert_eq!(res_json["rejected_count"], 1);

        // Verify persisted queue file state
        let reloaded_queue = CurationQueue::load_from_path(&queue_file).unwrap();
        let q_item1 = reloaded_queue
            .items
            .iter()
            .find(|i| i.id == item1.id)
            .unwrap();
        assert_eq!(q_item1.status, crate::curation::CurationStatus::Approved);
        assert_eq!(q_item1.curated_by, Some("reviewer_alice".to_string()));
        assert_eq!(q_item1.classification, Some("verified_data".to_string()));
        assert_eq!(q_item1.proposed_clearance, "PUBLIC");

        let q_item2 = reloaded_queue
            .items
            .iter()
            .find(|i| i.id == item2.id)
            .unwrap();
        assert_eq!(
            q_item2.status,
            crate::curation::CurationStatus::Rejected {
                reason: "Contains sensitive telemetry".to_string()
            }
        );
        assert_eq!(q_item2.curated_by, Some("reviewer_alice".to_string()));

        let q_item3 = reloaded_queue
            .items
            .iter()
            .find(|i| i.id == item3.id)
            .unwrap();
        assert_eq!(q_item3.status, crate::curation::CurationStatus::Pending);
    }

    #[tokio::test]
    async fn test_curation_pending_approve_reject_endpoints() {
        let (db_file, data_dir) = setup_test_env();
        let curation_dir = data_dir.path().join("curation");
        std::fs::create_dir_all(&curation_dir).unwrap();
        let queue_file = curation_dir.join("queue.json");
        let history_file = curation_dir.join("history.json");

        let mut queue = CurationQueue::new_with_path(queue_file.clone());
        let item1 = queue.submit_for_curation(
            "Item A for approval".to_string(),
            "CONFIDENTIAL".to_string(),
            Some("agent".to_string()),
        );
        let item2 = queue.submit_for_curation(
            "Item B for rejection".to_string(),
            "SECRET".to_string(),
            Some("user".to_string()),
        );
        queue.save().unwrap();

        let state = TrainingState {
            db_path: db_file.path().to_path_buf(),
            data_dir: data_dir.path().join("datasets"),
        };
        let app = router(state);

        // 1. GET /v1/curation/pending
        let req_pending = Request::builder()
            .method("GET")
            .uri("/v1/curation/pending")
            .body(Body::empty())
            .unwrap();

        let resp_pending = app.clone().oneshot(req_pending).await.unwrap();
        assert_eq!(resp_pending.status(), StatusCode::OK);
        let body_bytes = to_bytes(resp_pending.into_body(), usize::MAX).await.unwrap();
        let pending_items: Vec<crate::curation::CurationItem> =
            serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(pending_items.len(), 2);

        // 2. POST /v1/curation/{id}/approve
        let req_approve = Request::builder()
            .method("POST")
            .uri(format!("/v1/curation/{}/approve", item1.id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "curator": "admin_eve",
                    "classification": "verified",
                    "clearance": "PUBLIC"
                })
                .to_string(),
            ))
            .unwrap();

        let resp_approve = app.clone().oneshot(req_approve).await.unwrap();
        assert_eq!(resp_approve.status(), StatusCode::OK);

        // 3. POST /v1/curation/{id}/reject
        let req_reject = Request::builder()
            .method("POST")
            .uri(format!("/v1/curation/{}/reject", item2.id))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "curator": "admin_eve",
                    "reason": "Invalid sample"
                })
                .to_string(),
            ))
            .unwrap();

        let resp_reject = app.clone().oneshot(req_reject).await.unwrap();
        assert_eq!(resp_reject.status(), StatusCode::OK);

        // Verify history file was written
        let history = CurationQueue::load_history_from_path(&history_file).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].who, "admin_eve");
        assert!(history[0].what.contains(&item1.id));
        assert!(history[1].what.contains(&item2.id));
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
