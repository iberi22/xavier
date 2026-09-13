//! Axum REST handlers for Human Introspection & Pre-Training Refinement
//! Route prefix: `/v1/maloca/introspection/*`
//!
//! Exposes the IntrospectionEngine via HTTP so that users and frontends
//! can interactively refine and deep-think challenges before training bundle generation.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::humanchallenge::{
    types::{ChallengeType, IntrospectionSession, IntrospectionTechnique},
    HumanChallengeStore, IntrospectionEngine,
};

/// Shared state for Introspection handlers.
#[derive(Clone)]
pub struct IntrospectionState {
    pub engine: Arc<IntrospectionEngine>,
    pub store: Arc<HumanChallengeStore>,
}

impl IntrospectionState {
    pub fn new(store: Arc<HumanChallengeStore>) -> Self {
        let engine = Arc::new(IntrospectionEngine::new(store.clone()));
        Self { engine, store }
    }

    /// Creates an in-memory `IntrospectionState` for testing.
    pub fn in_memory() -> Self {
        let store = Arc::new(
            HumanChallengeStore::in_memory()
                .expect("failed to create in-memory HumanChallengeStore"),
        );
        Self::new(store)
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize)]
pub struct StartIntrospectionRequest {
    pub challenge_id: String,
    pub challenge_type: ChallengeType,
    pub description: String,
    pub technique: Option<IntrospectionTechnique>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessTurnRequest {
    pub human_input: String,
    pub challenge_description: String,
}

#[derive(Debug, Serialize)]
pub struct IntrospectionListResponse {
    pub total: usize,
    pub sessions: Vec<IntrospectionSession>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /v1/maloca/introspection/start`: Starts a new introspection session.
pub async fn start_introspection_handler(
    State(state): State<IntrospectionState>,
    Json(payload): Json<StartIntrospectionRequest>,
) -> impl IntoResponse {
    match state.engine.start_session(
        &payload.challenge_id,
        payload.challenge_type,
        &payload.description,
        payload.technique,
    ) {
        Ok(session) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "session": session })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// `POST /v1/maloca/introspection/{id}/turn`: Submits a human reply and receives the next guide prompt.
pub async fn process_turn_handler(
    State(state): State<IntrospectionState>,
    Path(session_id): Path<String>,
    Json(payload): Json<ProcessTurnRequest>,
) -> impl IntoResponse {
    match state.engine.process_turn(
        &session_id,
        &payload.human_input,
        &payload.challenge_description,
    ) {
        Ok(session) => (
            StatusCode::OK,
            Json(serde_json::json!({ "session": session })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// `POST /v1/maloca/introspection/{id}/complete`: Explicitly finalizes a session and extracts training insights.
pub async fn complete_introspection_handler(
    State(state): State<IntrospectionState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.engine.complete_session(&session_id) {
        Ok(session) => (
            StatusCode::OK,
            Json(serde_json::json!({ "session": session })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

/// `GET /v1/maloca/introspection/{id}`: Retrieves the current state of an introspection session.
pub async fn get_introspection_handler(
    State(state): State<IntrospectionState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_introspection_session(&session_id) {
        Ok(Some(session)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "session": session })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Introspection session not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(state: IntrospectionState) -> Router {
    Router::new()
        .route(
            "/v1/maloca/introspection/start",
            post(start_introspection_handler),
        )
        .route(
            "/v1/maloca/introspection/{id}/turn",
            post(process_turn_handler),
        )
        .route(
            "/v1/maloca/introspection/{id}/complete",
            post(complete_introspection_handler),
        )
        .route(
            "/v1/maloca/introspection/{id}",
            get(get_introspection_handler),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_start_session() {
        let state = IntrospectionState::in_memory();
        let app = router(state);

        let req_payload = StartIntrospectionRequest {
            challenge_id: "chal_test_start".to_string(),
            challenge_type: ChallengeType::Decision,
            description: "Choosing between RocksDB and SQLite WAL mode".to_string(),
            technique: Some(IntrospectionTechnique::PreMortem),
        };

        let req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/introspection/start")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&req_payload).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let res: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let session = &res["session"];
        assert!(session["id"].as_str().unwrap().starts_with("is_"));
        assert_eq!(session["challenge_id"], "chal_test_start");
        assert_eq!(session["technique"], "pre_mortem");
        assert_eq!(session["status"], "active");

        // The session starts with an opening guide turn
        let turns = session["turns"].as_array().unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0]["role"], "llm_guide");
    }

    #[tokio::test]
    async fn test_process_turn() {
        let state = IntrospectionState::in_memory();
        let app = router(state);

        // 1. Start session
        let start_payload = StartIntrospectionRequest {
            challenge_id: "chal_test_turn".to_string(),
            challenge_type: ChallengeType::Contradiction,
            description: "Async queued pipeline vs synchronous writes".to_string(),
            technique: Some(IntrospectionTechnique::SocraticQuestioning),
        };

        let start_req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/introspection/start")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&start_payload).unwrap()))
            .unwrap();

        let start_resp = app.clone().oneshot(start_req).await.unwrap();
        assert_eq!(start_resp.status(), StatusCode::CREATED);

        let body = to_bytes(start_resp.into_body(), usize::MAX).await.unwrap();
        let start_res: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = start_res["session"]["id"].as_str().unwrap().to_string();

        // 2. Submit a turn
        let turn_payload = ProcessTurnRequest {
            human_input: "We prioritize data consistency over raw latency for financial audits."
                .to_string(),
            challenge_description: "Async queued pipeline vs synchronous writes".to_string(),
        };

        let turn_req = Request::builder()
            .method("POST")
            .uri(format!("/v1/maloca/introspection/{}/turn", session_id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&turn_payload).unwrap()))
            .unwrap();

        let turn_resp = app.oneshot(turn_req).await.unwrap();
        assert_eq!(turn_resp.status(), StatusCode::OK);

        let turn_body = to_bytes(turn_resp.into_body(), usize::MAX).await.unwrap();
        let turn_res: serde_json::Value = serde_json::from_slice(&turn_body).unwrap();

        let session = &turn_res["session"];
        assert_eq!(session["id"], session_id);
        let turns = session["turns"].as_array().unwrap();
        // Opening guide turn + human turn + new guide turn = 3 turns
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[1]["role"], "human");
        assert_eq!(
            turns[1]["content"],
            "We prioritize data consistency over raw latency for financial audits."
        );
        assert_eq!(turns[2]["role"], "llm_guide");
        assert!(session["depth_score"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    async fn test_complete_session() {
        let state = IntrospectionState::in_memory();
        let app = router(state);

        // 1. Start session
        let start_payload = StartIntrospectionRequest {
            challenge_id: "chal_test_complete".to_string(),
            challenge_type: ChallengeType::Execution,
            description: "High cache miss rate under concurrency".to_string(),
            technique: Some(IntrospectionTechnique::FiveWhys),
        };

        let start_req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/introspection/start")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&start_payload).unwrap()))
            .unwrap();

        let start_resp = app.clone().oneshot(start_req).await.unwrap();
        let body = to_bytes(start_resp.into_body(), usize::MAX).await.unwrap();
        let start_res: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = start_res["session"]["id"].as_str().unwrap().to_string();

        // 2. Add a human turn first
        let turn_payload = ProcessTurnRequest {
            human_input: "Keys were evicted early due to aggressive TTL".to_string(),
            challenge_description: "High cache miss rate under concurrency".to_string(),
        };

        let turn_req = Request::builder()
            .method("POST")
            .uri(format!("/v1/maloca/introspection/{}/turn", session_id))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&turn_payload).unwrap()))
            .unwrap();

        let _ = app.clone().oneshot(turn_req).await.unwrap();

        // 3. Complete session
        let complete_req = Request::builder()
            .method("POST")
            .uri(format!("/v1/maloca/introspection/{}/complete", session_id))
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();

        let complete_resp = app.oneshot(complete_req).await.unwrap();
        assert_eq!(complete_resp.status(), StatusCode::OK);

        let complete_body = to_bytes(complete_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let complete_res: serde_json::Value = serde_json::from_slice(&complete_body).unwrap();

        let session = &complete_res["session"];
        assert_eq!(session["id"], session_id);
        assert_eq!(session["status"], "completed");
        assert!(session["completed_at"].is_string());
        assert!(session["depth_score"].as_f64().unwrap() > 0.0);
    }

    #[tokio::test]
    async fn test_get_session() {
        let state = IntrospectionState::in_memory();
        let app = router(state);

        // 1. Start session
        let start_payload = StartIntrospectionRequest {
            challenge_id: "chal_test_get".to_string(),
            challenge_type: ChallengeType::Assumption,
            description: "Assumed network is never partitioned".to_string(),
            technique: None, // Auto-selected technique (Socratic for Assumption)
        };

        let start_req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/introspection/start")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&start_payload).unwrap()))
            .unwrap();

        let start_resp = app.clone().oneshot(start_req).await.unwrap();
        let body = to_bytes(start_resp.into_body(), usize::MAX).await.unwrap();
        let start_res: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = start_res["session"]["id"].as_str().unwrap().to_string();

        // 2. GET existing session
        let get_req = Request::builder()
            .method("GET")
            .uri(format!("/v1/maloca/introspection/{}", session_id))
            .body(Body::empty())
            .unwrap();

        let get_resp = app.clone().oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), StatusCode::OK);

        let get_body = to_bytes(get_resp.into_body(), usize::MAX).await.unwrap();
        let get_res: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_res["session"]["id"], session_id);
        assert_eq!(get_res["session"]["challenge_id"], "chal_test_get");
        assert_eq!(get_res["session"]["technique"], "socratic_questioning");

        // 3. GET nonexistent session -> 404
        let get_nonexistent_req = Request::builder()
            .method("GET")
            .uri("/v1/maloca/introspection/is_nonexistent_999")
            .body(Body::empty())
            .unwrap();

        let not_found_resp = app.oneshot(get_nonexistent_req).await.unwrap();
        assert_eq!(not_found_resp.status(), StatusCode::NOT_FOUND);
    }
}
