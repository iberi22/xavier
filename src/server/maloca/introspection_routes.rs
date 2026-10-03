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
    store::VerifyVoteError,
    types::{
        ChallengeType, CurationVerdict, CurationVote, IntrospectionSession, IntrospectionTechnique,
    },
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

/// Optional body of `POST /v1/maloca/introspection/{id}/complete`.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct CompleteIntrospectionRequest {
    /// Explicit consent to use the insights for model training (default false).
    #[serde(default)]
    pub training_consent: bool,
}

#[derive(Debug, Serialize)]
pub struct IntrospectionListResponse {
    pub total: usize,
    pub sessions: Vec<IntrospectionSession>,
}

/// Body of `POST /v1/maloca/introspection/votes/{id}/verify`.
#[derive(Debug, Deserialize, Serialize)]
pub struct VerifyVoteRequest {
    /// `accept` or `reject` (anything else is a 400).
    pub verdict: CurationVerdict,
    /// The human checked the insight against reality. Required for training use.
    #[serde(default)]
    pub fact_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct PendingVotesResponse {
    pub total: usize,
    pub votes: Vec<CurationVote>,
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
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // The body is optional: empty means defaults (no training consent).
    let payload: CompleteIntrospectionRequest = if body.iter().all(|b| b.is_ascii_whitespace()) {
        CompleteIntrospectionRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid body: {e}") })),
                )
                    .into_response()
            }
        }
    };
    match state
        .engine
        .complete_session_with_consent(&session_id, payload.training_consent)
    {
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

/// `GET /v1/maloca/introspection/pending-votes`: insights awaiting human verification.
pub async fn pending_votes_handler(State(state): State<IntrospectionState>) -> impl IntoResponse {
    match state.store.list_pending_introspection_votes(200) {
        Ok(votes) => (
            StatusCode::OK,
            Json(PendingVotesResponse {
                total: votes.len(),
                votes,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// `POST /v1/maloca/introspection/votes/{id}/verify`: records the human verdict on an
/// insight. Mutating verbs on `/v1/maloca/*` require the API token (see
/// `maloca_mutation_auth_middleware`).
pub async fn verify_vote_handler(
    State(state): State<IntrospectionState>,
    Path(vote_id): Path<String>,
    Json(payload): Json<VerifyVoteRequest>,
) -> impl IntoResponse {
    let accept = match payload.verdict {
        CurationVerdict::Accept => true,
        CurationVerdict::Reject => false,
        CurationVerdict::Refine => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "verdict must be accept or reject" })),
            )
                .into_response()
        }
    };
    match state
        .store
        .verify_introspection_vote(&vote_id, accept, payload.fact_verified)
    {
        Ok(vote) => (StatusCode::OK, Json(serde_json::json!({ "vote": vote }))).into_response(),
        Err(e) => {
            let status = match e {
                VerifyVoteError::NotFound => StatusCode::NOT_FOUND,
                VerifyVoteError::NotIntrospection | VerifyVoteError::AlreadyReviewed => {
                    StatusCode::CONFLICT
                }
                VerifyVoteError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, Json(serde_json::json!({ "error": e.to_string() }))).into_response()
        }
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
            "/v1/maloca/introspection/pending-votes",
            get(pending_votes_handler),
        )
        .route(
            "/v1/maloca/introspection/votes/{id}/verify",
            post(verify_vote_handler),
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

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> serde_json::Value {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert!(resp.status().is_success(), "{uri} -> {}", resp.status());
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// start -> turn -> complete(consent) over HTTP against a file-backed store.
    async fn run_flow(db: &std::path::Path, challenge: &str, consent: bool) -> String {
        let store = Arc::new(HumanChallengeStore::new(db).unwrap());
        let app = router(IntrospectionState::new(store));
        let started = call(
            &app,
            "POST",
            "/v1/maloca/introspection/start",
            serde_json::json!({
                "challenge_id": challenge,
                "challenge_type": "decision",
                "description": "Pick a storage engine",
                "technique": "pre_mortem",
            }),
        )
        .await;
        let id = started["session"]["id"].as_str().unwrap().to_string();
        call(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/{id}/turn"),
            serde_json::json!({
                "human_input": "We would lose data because the WAL checkpoint never runs under sustained load.",
                "challenge_description": "Pick a storage engine",
            }),
        )
        .await;
        let done = call(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/{id}/complete"),
            serde_json::json!({ "training_consent": consent }),
        )
        .await;
        assert_eq!(done["session"]["status"], "completed");
        assert_eq!(done["session"]["training_consent"], consent);
        id
    }

    #[tokio::test]
    async fn test_complete_with_consent_writes_eligible_votes_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("humanchallenge.db");
        let id = run_flow(&db, "chal_consent_yes", true).await;

        // Reopen the store from the same path: everything must have survived.
        let reopened = HumanChallengeStore::new(&db).unwrap();
        let votes = reopened
            .get_votes_for_challenge("chal_consent_yes")
            .unwrap();
        assert_eq!(votes.len(), 1);
        assert!(votes[0].training_eligible);
        assert_eq!(votes[0].technique.as_deref(), Some("pre_mortem"));
        // Unverified insights stay out of the training gate until a human verifies them.
        assert_eq!(reopened.count_training_eligible().unwrap(), 0);
        let session = reopened.get_introspection_session(&id).unwrap().unwrap();
        assert!(session.training_consent);
        assert_eq!(session.insights.len(), 1);
    }

    #[tokio::test]
    async fn test_complete_without_consent_writes_non_eligible_votes() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("humanchallenge.db");
        run_flow(&db, "chal_consent_no", false).await;

        let reopened = HumanChallengeStore::new(&db).unwrap();
        let votes = reopened.get_votes_for_challenge("chal_consent_no").unwrap();
        assert_eq!(votes.len(), 1);
        assert!(!votes[0].training_eligible);
        assert_eq!(reopened.count_training_eligible().unwrap(), 0);
    }

    async fn status_of(
        app: &Router,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    /// Two-turn flow so exactly one insight vote exists; returns (app, store, vote id).
    async fn app_with_pending_vote(consent: bool) -> (Router, Arc<HumanChallengeStore>, String) {
        let store = Arc::new(HumanChallengeStore::in_memory().unwrap());
        let app = router(IntrospectionState::new(store.clone()));
        let started = call(
            &app,
            "POST",
            "/v1/maloca/introspection/start",
            serde_json::json!({
                "challenge_id": "chal_verify",
                "challenge_type": "decision",
                "description": "Pick a storage engine",
                "technique": "pre_mortem",
            }),
        )
        .await;
        let id = started["session"]["id"].as_str().unwrap().to_string();
        call(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/{id}/turn"),
            serde_json::json!({
                "human_input": "We would lose data because the WAL checkpoint never runs under sustained load.",
                "challenge_description": "Pick a storage engine",
            }),
        )
        .await;
        call(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/{id}/complete"),
            serde_json::json!({ "training_consent": consent }),
        )
        .await;
        let votes = store.get_votes_for_challenge("chal_verify").unwrap();
        assert_eq!(votes.len(), 1);
        (app, store, votes[0].id.clone())
    }

    #[tokio::test]
    async fn test_pending_votes_listed_and_not_eligible() {
        let (app, store, vote_id) = app_with_pending_vote(true).await;
        let (st, body) = status_of(
            &app,
            "GET",
            "/v1/maloca/introspection/pending-votes",
            serde_json::json!({}),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(body["total"], 1);
        assert_eq!(body["votes"][0]["id"], vote_id);
        assert_eq!(store.count_training_eligible().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_verify_accept_with_consent_becomes_eligible() {
        let (app, store, vote_id) = app_with_pending_vote(true).await;
        let (st, body) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "accept", "fact_verified": true }),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert_eq!(store.count_training_eligible().unwrap(), 1);
        // No longer pending.
        assert!(store
            .list_pending_introspection_votes(10)
            .unwrap()
            .is_empty());
        // A second verification is refused.
        let (st, _) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "reject" }),
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn test_verify_accept_without_consent_stays_ineligible() {
        let (app, store, vote_id) = app_with_pending_vote(false).await;
        let (st, _) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "accept", "fact_verified": true }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(store.count_training_eligible().unwrap(), 0);
        let vote = &store.get_votes_for_challenge("chal_verify").unwrap()[0];
        assert!(vote.fact_verified);
        assert!(!vote.training_eligible);
    }

    #[tokio::test]
    async fn test_verify_accept_without_fact_verified_stays_ineligible() {
        let (app, store, vote_id) = app_with_pending_vote(true).await;
        let (st, _) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "accept", "fact_verified": false }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(store.count_training_eligible().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_verify_reject_is_excluded() {
        let (app, store, vote_id) = app_with_pending_vote(true).await;
        let (st, _) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "reject", "fact_verified": true }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(store.count_training_eligible().unwrap(), 0);
        assert!(store
            .list_pending_introspection_votes(10)
            .unwrap()
            .is_empty());
        let vote = &store.get_votes_for_challenge("chal_verify").unwrap()[0];
        assert_eq!(vote.verdict, CurationVerdict::Reject);
        assert!(!vote.training_eligible && !vote.fact_verified);
    }

    #[tokio::test]
    async fn test_verify_rejects_refine_verdict_and_unknown_vote() {
        let (app, _store, vote_id) = app_with_pending_vote(true).await;
        let (st, _) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "refine" }),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _) = status_of(
            &app,
            "POST",
            "/v1/maloca/introspection/votes/cv_missing/verify",
            serde_json::json!({ "verdict": "accept", "fact_verified": true }),
        )
        .await;
        assert_eq!(st, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_verify_leaves_non_introspection_votes_and_other_domains_alone() {
        use crate::humanchallenge::curation_gate::{evaluate_domains, ReadinessThresholds};
        let (app, store, vote_id) = app_with_pending_vote(true).await;
        // A plain human curation vote in another domain, already eligible.
        let human = CurationVote::new(
            "chal_other",
            CurationVerdict::Accept,
            Some("rust is memory safe".into()),
            true,
            vec!["rust".into()],
            true,
        );
        store.save_curation_vote(&human).unwrap();
        let ready_before = |store: &HumanChallengeStore| {
            let votes = store.get_training_eligible_votes(100).unwrap();
            evaluate_domains(
                &ReadinessThresholds {
                    min_examples_per_domain: 1,
                    ..Default::default()
                },
                &votes,
                &[],
            )
            .into_iter()
            .map(|d| d.domain)
            .collect::<Vec<_>>()
        };
        assert_eq!(ready_before(&store), vec!["rust".to_string()]);

        // Verifying a non-introspection vote is refused and leaves it intact.
        let (st, _) = status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{}/verify", human.id),
            serde_json::json!({ "verdict": "reject" }),
        )
        .await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(store.count_training_eligible().unwrap(), 1);

        // Verifying the insight adds the untagged (general) domain, only now.
        status_of(
            &app,
            "POST",
            &format!("/v1/maloca/introspection/votes/{vote_id}/verify"),
            serde_json::json!({ "verdict": "accept", "fact_verified": true }),
        )
        .await;
        let after = ready_before(&store);
        assert!(after.contains(&"rust".to_string()));
        assert_eq!(after.len(), 2);
    }
}
