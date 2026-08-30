//! Orion Health clinical record and share pass routes for Xavier.
//!
//! Provides time-locked (`consultation_ttl`) and single-use (`read_once`) access control
//! endpoints for family health records encrypted with a family key.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared state for Orion Health clinical records and share passes.
#[derive(Clone, Default)]
pub struct MeshHealthState {
    pub store: Arc<Mutex<HealthStore>>,
}

impl MeshHealthState {
    /// Creates a new instance of MeshHealthState.
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HealthStore::default())),
        }
    }
}

/// In-memory storage for records and active share passes.
#[derive(Default)]
pub struct HealthStore {
    pub records: HashMap<String, ClinicalRecord>,
    pub passes: HashMap<String, SharePass>,
}

/// Clinical record structure representing an encrypted family health document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClinicalRecord {
    pub id: String,
    pub family_id: String,
    pub record_type: String,
    pub patient_id: Option<String>,
    pub encrypted_payload: String,
    pub family_key_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Time-locked and/or read-once access pass token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePass {
    pub pass_token: String,
    pub record_id: String,
    pub doctor_id: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub read_once: bool,
    pub used: bool,
    pub created_at: DateTime<Utc>,
}

// ---------- Request / Response DTOs ----------

/// Request payload to save a clinical record.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SaveRecordRequest {
    pub family_id: String,
    pub record_type: Option<String>,
    pub patient_id: Option<String>,
    pub encrypted_payload: String,
    pub family_key_id: Option<String>,
}

/// Response payload after saving a clinical record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveRecordResponse {
    pub id: String,
    pub family_id: String,
    pub status: String,
    pub created_at: String,
}

/// Request payload to generate a share pass.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateSharePassRequest {
    pub doctor_id: Option<String>,
    pub consultation_ttl: Option<u64>,
    pub read_once: Option<bool>,
}

/// Response payload containing share pass details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSharePassResponse {
    pub pass_token: String,
    pub record_id: String,
    pub doctor_id: Option<String>,
    pub expires_at: Option<String>,
    pub read_once: bool,
    pub created_at: String,
}

/// Query parameters for viewing a record using a share pass token.
#[derive(Debug, Clone, Deserialize)]
pub struct ViewRecordParams {
    pub pass_token: Option<String>,
    pub token: Option<String>,
}

/// Response payload when viewing a clinical record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewRecordResponse {
    pub id: String,
    pub family_id: String,
    pub record_type: String,
    pub patient_id: Option<String>,
    pub encrypted_payload: String,
    pub family_key_id: Option<String>,
    pub created_at: String,
    pub pass_status: ViewPassStatus,
}

/// Share pass status details embedded in the view response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewPassStatus {
    pub pass_token: String,
    pub read_once: bool,
    pub used: bool,
    pub expires_at: Option<String>,
}

// ---------- Handlers ----------

/// POST /v1/mesh/health/records
/// Saves family clinical record encrypted with family key.
pub async fn save_record_handler(
    State(state): State<MeshHealthState>,
    Json(req): Json<SaveRecordRequest>,
) -> impl IntoResponse {
    if req.family_id.trim().is_empty() || req.encrypted_payload.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "family_id and encrypted_payload are required",
        )
            .into_response();
    }

    let record_id = format!("rec_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now();

    let record = ClinicalRecord {
        id: record_id.clone(),
        family_id: req.family_id.clone(),
        record_type: req
            .record_type
            .unwrap_or_else(|| "general_clinical".to_string()),
        patient_id: req.patient_id,
        encrypted_payload: req.encrypted_payload,
        family_key_id: req.family_key_id,
        created_at: now,
    };

    let mut store = state.store.lock().unwrap();
    store.records.insert(record_id.clone(), record);

    (
        StatusCode::CREATED,
        Json(SaveRecordResponse {
            id: record_id,
            family_id: req.family_id,
            status: "saved".to_string(),
            created_at: now.to_rfc3339(),
        }),
    )
        .into_response()
}

/// POST /v1/mesh/health/records/:id/share-pass
/// Generates time-locked or read-once access token for doctors.
pub async fn create_share_pass_handler(
    State(state): State<MeshHealthState>,
    Path(id): Path<String>,
    Json(req): Json<CreateSharePassRequest>,
) -> impl IntoResponse {
    let mut store = state.store.lock().unwrap();
    if !store.records.contains_key(&id) {
        return (StatusCode::NOT_FOUND, "Record not found").into_response();
    }

    let now = Utc::now();
    let pass_token = format!("pass_{}", uuid::Uuid::new_v4().simple());
    let expires_at = req
        .consultation_ttl
        .map(|ttl_secs| now + chrono::Duration::seconds(ttl_secs as i64));
    let read_once = req.read_once.unwrap_or(false);

    let pass = SharePass {
        pass_token: pass_token.clone(),
        record_id: id.clone(),
        doctor_id: req.doctor_id.clone(),
        expires_at,
        read_once,
        used: false,
        created_at: now,
    };

    store.passes.insert(pass_token.clone(), pass);

    (
        StatusCode::CREATED,
        Json(CreateSharePassResponse {
            pass_token,
            record_id: id,
            doctor_id: req.doctor_id,
            expires_at: expires_at.map(|dt| dt.to_rfc3339()),
            read_once,
            created_at: now.to_rfc3339(),
        }),
    )
        .into_response()
}

/// GET /v1/mesh/health/records/:id/view
/// Verifies pass validity, marks as read if read-once, and returns record.
pub async fn view_record_handler(
    State(state): State<MeshHealthState>,
    Path(id): Path<String>,
    Query(params): Query<ViewRecordParams>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Extract pass token from query params or headers
    let pass_token = params
        .pass_token
        .or(params.token)
        .or_else(|| {
            headers
                .get("x-share-pass")
                .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
        })
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|auth| auth.strip_prefix("Bearer "))
                .map(|s| s.trim().to_string())
        });

    let pass_token = match pass_token {
        Some(t) if !t.trim().is_empty() => t,
        _ => return (StatusCode::UNAUTHORIZED, "Missing share pass token").into_response(),
    };

    let mut store = state.store.lock().unwrap();

    let record = match store.records.get(&id) {
        Some(r) => r.clone(),
        None => return (StatusCode::NOT_FOUND, "Record not found").into_response(),
    };

    let pass = match store.passes.get_mut(&pass_token) {
        Some(p) if p.record_id == id => p,
        _ => return (StatusCode::FORBIDDEN, "Invalid share pass token").into_response(),
    };

    let now = Utc::now();

    // Check expiration if consultation_ttl was set
    if let Some(expires_at) = pass.expires_at {
        if now > expires_at {
            return (StatusCode::FORBIDDEN, "Share pass has expired").into_response();
        }
    }

    // Anti-hallucination guard: Automatic invalidation after first view when read_once is true
    if pass.read_once && pass.used {
        return (
            StatusCode::FORBIDDEN,
            "Share pass already consumed (read-once)",
        )
            .into_response();
    }

    if pass.read_once {
        pass.used = true;
    }

    let response = ViewRecordResponse {
        id: record.id,
        family_id: record.family_id,
        record_type: record.record_type,
        patient_id: record.patient_id,
        encrypted_payload: record.encrypted_payload,
        family_key_id: record.family_key_id,
        created_at: record.created_at.to_rfc3339(),
        pass_status: ViewPassStatus {
            pass_token: pass.pass_token.clone(),
            read_once: pass.read_once,
            used: pass.used,
            expires_at: pass.expires_at.map(|dt| dt.to_rfc3339()),
        },
    };

    (StatusCode::OK, Json(response)).into_response()
}

// ---------- Router ----------

/// Builds the Axum router for Orion Health mesh endpoints.
pub fn router(state: MeshHealthState) -> Router {
    Router::new()
        .route("/v1/mesh/health/records", post(save_record_handler))
        .route(
            "/v1/mesh/health/records/{id}/share-pass",
            post(create_share_pass_handler),
        )
        .route("/v1/mesh/health/records/{id}/view", get(view_record_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_mesh_health_time_locked_pass() {
        let state = MeshHealthState::new();
        let app = router(state.clone());

        // 1. Save clinical record
        let save_req = r#"{
            "family_id": "fam_smith_99",
            "record_type": "consultation_notes",
            "patient_id": "pat_john_doe",
            "encrypted_payload": "AES256GCM:EncryptedPayloadDataBlob12345",
            "family_key_id": "key_v1"
        }"#;

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/mesh/health/records")
                    .header("content-type", "application/json")
                    .body(Body::from(save_req))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let save_res: SaveRecordResponse = serde_json::from_slice(&body).unwrap();
        let record_id = save_res.id;
        assert_eq!(save_res.family_id, "fam_smith_99");

        // 2. Generate time-locked & read-once share pass
        let pass_req = r#"{
            "doctor_id": "doc_house_42",
            "consultation_ttl": 3600,
            "read_once": true
        }"#;

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/mesh/health/records/{}/share-pass", record_id))
                    .header("content-type", "application/json")
                    .body(Body::from(pass_req))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let pass_res: CreateSharePassResponse = serde_json::from_slice(&body).unwrap();
        let pass_token = pass_res.pass_token;
        assert!(pass_res.read_once);
        assert!(pass_res.expires_at.is_some());

        // 3. First view: succeeds and invalidates pass because read_once = true
        let view_uri = format!(
            "/v1/mesh/health/records/{}/view?pass_token={}",
            record_id, pass_token
        );
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&view_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let view_res: ViewRecordResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            view_res.encrypted_payload,
            "AES256GCM:EncryptedPayloadDataBlob12345"
        );
        assert!(view_res.pass_status.used);

        // 4. Second view with read_once pass: rejected (Forbidden)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&view_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // 5. Generate expired pass (consultation_ttl: 0)
        let expired_pass_req = r#"{
            "doctor_id": "doc_strange_7",
            "consultation_ttl": 0,
            "read_once": false
        }"#;

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/mesh/health/records/{}/share-pass", record_id))
                    .header("content-type", "application/json")
                    .body(Body::from(expired_pass_req))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let exp_pass_res: CreateSharePassResponse = serde_json::from_slice(&body).unwrap();

        // Give small sleep to guarantee expiration check passes now > expires_at
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let exp_view_uri = format!(
            "/v1/mesh/health/records/{}/view?pass_token={}",
            record_id, exp_pass_res.pass_token
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(&exp_view_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
