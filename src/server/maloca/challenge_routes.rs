//! Axum REST handlers for Human Challenge Interaction (`/v1/maloca/challenges/*`).
//!
//! Provides HTTP endpoints for generating, answering, listing, and querying stats
//! for HumanChallenge events stored in the node's local SQLite database.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::humanchallenge::{
    ChallengeStatus, ChallengeType, HumanChallengeEvent, HumanChallengeStore,
};

/// Shared state for HumanChallenge Axum handlers.
#[derive(Clone)]
pub struct ChallengeState {
    pub store: Arc<HumanChallengeStore>,
}

impl ChallengeState {
    /// Creates a new `ChallengeState` wrapping a `HumanChallengeStore`.
    pub fn new(store: Arc<HumanChallengeStore>) -> Self {
        Self { store }
    }

    /// Creates an in-memory `ChallengeState` for testing.
    pub fn in_memory() -> Self {
        let store = HumanChallengeStore::in_memory()
            .expect("failed to create in-memory HumanChallengeStore");
        Self {
            store: Arc::new(store),
        }
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

/// Request payload for generating challenges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerateChallengeRequest {
    /// Target session ID (defaults to "default_session" if omitted).
    pub session_id: Option<String>,
    /// Optional filter to generate only a specific challenge type.
    pub challenge_type: Option<ChallengeType>,
}

/// Response payload for generated challenges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateChallengeResponse {
    pub count: usize,
    pub challenges: Vec<HumanChallengeEvent>,
}

/// Request payload for answering a challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerChallengeRequest {
    pub challenge_id: String,
    pub response: String,
}

/// Response payload after answering a challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerChallengeResponse {
    pub challenge_id: String,
    pub status: ChallengeStatus,
    pub score: f32,
    pub points_awarded: u32,
    pub trust_points: u32,
    pub verified: bool,
    pub message: String,
}

/// Query parameters for listing challenges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListChallengesQuery {
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub limit: Option<u32>,
}

/// Response payload for listing challenges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChallengesResponse {
    pub session_id: Option<String>,
    pub total: usize,
    pub challenges: Vec<HumanChallengeEvent>,
}

/// Query parameters for challenge statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatsQuery {
    pub year_month: Option<String>,
}

/// Response payload for challenge statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeStatsResponse {
    pub total_challenges: usize,
    pub answered_count: usize,
    pub verified_count: usize,
    pub accuracy: f32,
    pub completion_rate: f32,
    pub x2_points_total: u32,
    pub x2_target_points: u32,
    pub x2_points_breakdown: HashMap<String, u32>,
}

// ---------------------------------------------------------------------------
// HcAnalyzerBridge Helper
// ---------------------------------------------------------------------------

/// Internal analyzer bridge for scoring human challenge responses and awarding trust points.
pub struct HcAnalyzerBridge;

impl HcAnalyzerBridge {
    /// Evaluates a user response against a challenge event, returning (score, points_awarded, trust_points).
    pub fn evaluate(event: &HumanChallengeEvent, response_text: &str) -> (f32, u32, u32) {
        let trimmed = response_text.trim();
        if trimmed.is_empty() {
            return (0.0, 0, 0);
        }

        let length_quality = if trimmed.len() >= 15 {
            1.0
        } else if trimmed.len() >= 5 {
            0.8
        } else {
            0.5
        };

        let score = (event.confidence_score * length_quality).clamp(0.0, 1.0);
        let base_points = (score * 10.0).round() as u32;
        let trust_points = if score >= 0.7 {
            base_points * 2
        } else {
            base_points
        };

        (score, base_points, trust_points)
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /v1/maloca/challenges/generate`: Deterministically generates candidate challenges.
pub async fn generate_challenges_handler(
    State(state): State<ChallengeState>,
    Json(payload): Json<Option<GenerateChallengeRequest>>,
) -> impl IntoResponse {
    let req = payload.unwrap_or_default();
    let session_id = req
        .session_id
        .unwrap_or_else(|| "default_session".to_string());

    let types_to_generate = if let Some(ct) = req.challenge_type {
        vec![ct]
    } else {
        vec![
            ChallengeType::Contradiction,
            ChallengeType::Decision,
            ChallengeType::Execution,
            ChallengeType::Assumption,
            ChallengeType::Clarification,
        ]
    };

    let mut generated = Vec::new();

    for ct in types_to_generate {
        let (desc, content, conf) = match ct {
            ChallengeType::Contradiction => (
                "Conflicting instructions detected in requirement set",
                "Requirement A specifies synchronous write while B mandates asynchronous queued pipeline.",
                0.90,
            ),
            ChallengeType::Decision => (
                "Architecture decision requiring human confirmation",
                "Choosing between SQLite WAL mode with shared cache vs isolated connection pools.",
                0.85,
            ),
            ChallengeType::Execution => (
                "Critical tool execution approval needed",
                "Execution script modifying production SQLite tables requires human approval.",
                0.95,
            ),
            ChallengeType::Assumption => (
                "Implicit assumption detected in reasoning",
                "Assumed peer network connectivity is persistent throughout offline buffer replay.",
                0.80,
            ),
            ChallengeType::Clarification => (
                "Ambiguous prompt requires human clarification",
                "Query requested 'update state' without specifying workspace or project ID.",
                0.75,
            ),
        };

        let event = HumanChallengeEvent::new(&session_id, ct, desc, content, conf);
        if let Err(e) = state.store.save_event(&event) {
            tracing::warn!(
                "Failed to save generated challenge event {}: {}",
                event.id,
                e
            );
        }
        generated.push(event);
    }

    let response = GenerateChallengeResponse {
        count: generated.len(),
        challenges: generated,
    };

    (StatusCode::OK, Json(response))
}

/// `POST /v1/maloca/challenges/answer`: Scores response using `HcAnalyzerBridge` and awards trust points.
pub async fn answer_challenge_handler(
    State(state): State<ChallengeState>,
    Json(payload): Json<AnswerChallengeRequest>,
) -> impl IntoResponse {
    let event = match state.store.get_event_by_id(&payload.challenge_id) {
        Ok(Some(ev)) => ev,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Challenge not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if event.status != ChallengeStatus::Candidate {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Challenge is already in status '{}'", event.status.as_str())
            })),
        )
            .into_response();
    }

    let (score, points_awarded, trust_points) =
        HcAnalyzerBridge::evaluate(&event, &payload.response);

    let updated = state
        .store
        .answer_challenge(&event.id, &payload.response, points_awarded)
        .unwrap_or(false);

    let verified = score >= 0.7;
    let status = if verified {
        ChallengeStatus::Verified
    } else {
        ChallengeStatus::Answered
    };

    let message = if updated {
        if verified {
            format!(
                "Response verified with score {:.2}. Awarded {} points and {} trust points.",
                score, points_awarded, trust_points
            )
        } else {
            format!(
                "Response recorded with score {:.2}. Awarded {} points.",
                score, points_awarded
            )
        }
    } else {
        "Failed to record response".to_string()
    };

    let response = AnswerChallengeResponse {
        challenge_id: payload.challenge_id,
        status,
        score,
        points_awarded,
        trust_points,
        verified,
        message,
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// `GET /v1/maloca/challenges/list`: Lists active challenges for current node session.
pub async fn list_challenges_handler(
    State(state): State<ChallengeState>,
    Query(query): Query<ListChallengesQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);

    let status_filter = query
        .status
        .as_deref()
        .and_then(|s| match s.to_lowercase().as_str() {
            "candidate" => Some(ChallengeStatus::Candidate),
            "answered" => Some(ChallengeStatus::Answered),
            "verified" => Some(ChallengeStatus::Verified),
            "rejected" => Some(ChallengeStatus::Rejected),
            "expired" => Some(ChallengeStatus::Expired),
            _ => None,
        });

    let events = match state.store.list_events(status_filter, limit) {
        Ok(evs) => evs,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let filtered_events: Vec<HumanChallengeEvent> = if let Some(ref sid) = query.session_id {
        events
            .into_iter()
            .filter(|e| &e.session_id == sid)
            .collect()
    } else {
        events
    };

    let total = filtered_events.len();
    let response = ListChallengesResponse {
        session_id: query.session_id,
        total,
        challenges: filtered_events,
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// `GET /v1/maloca/challenges/stats`: Returns accuracy, completion rate, and X2 points breakdown.
pub async fn stats_handler(
    State(state): State<ChallengeState>,
    Query(query): Query<StatsQuery>,
) -> impl IntoResponse {
    let year_month = query
        .year_month
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m").to_string());

    let farming_summary = match state.store.get_farming_summary(&year_month) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let month_events = state
        .store
        .list_events_by_month(&year_month, 1000)
        .unwrap_or_default();

    let total_challenges = month_events.len();
    let answered_count = farming_summary.answered_count as usize;
    let verified_count = farming_summary.verified_count as usize;

    let completion_rate = if total_challenges > 0 {
        answered_count as f32 / total_challenges as f32
    } else {
        0.0
    };

    let accuracy = if answered_count > 0 {
        verified_count as f32 / answered_count as f32
    } else {
        0.0
    };

    let mut x2_points_breakdown = HashMap::new();
    for event in &month_events {
        let key = event.challenge_type.as_str().to_string();
        *x2_points_breakdown.entry(key).or_insert(0) += event.points_awarded;
    }

    let response = ChallengeStatsResponse {
        total_challenges,
        answered_count,
        verified_count,
        accuracy,
        completion_rate,
        x2_points_total: farming_summary.total_points,
        x2_target_points: farming_summary.target_points,
        x2_points_breakdown,
    };

    (StatusCode::OK, Json(response)).into_response()
}

// ---------------------------------------------------------------------------
// Router Constructor
// ---------------------------------------------------------------------------

/// Constructs the Axum Router for Human Challenge endpoints under `/v1/maloca/challenges`.
pub fn router(state: ChallengeState) -> Router {
    Router::new()
        .route(
            "/v1/maloca/challenges/generate",
            post(generate_challenges_handler),
        )
        .route(
            "/v1/maloca/challenges/answer",
            post(answer_challenge_handler),
        )
        .route("/v1/maloca/challenges/list", get(list_challenges_handler))
        .route("/v1/maloca/challenges/stats", get(stats_handler))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_generate_challenges_endpoint() {
        let state = ChallengeState::in_memory();
        let app = router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/challenges/generate")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "session_id": "session_test_1"
                })
                .to_string(),
            ))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let res: GenerateChallengeResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(res.count, 5);
        assert_eq!(res.challenges.len(), 5);

        let types: Vec<_> = res.challenges.iter().map(|c| c.challenge_type).collect();
        assert!(types.contains(&ChallengeType::Contradiction));
        assert!(types.contains(&ChallengeType::Decision));
        assert!(types.contains(&ChallengeType::Execution));
        assert!(types.contains(&ChallengeType::Assumption));
        assert!(types.contains(&ChallengeType::Clarification));
    }

    #[tokio::test]
    async fn test_answer_challenge_endpoint() {
        let state = ChallengeState::in_memory();
        let app = router(state.clone());

        // Generate challenges first
        let gen_req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/challenges/generate")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "session_id": "session_test_answer"
                })
                .to_string(),
            ))
            .unwrap();
        let gen_resp = app.clone().oneshot(gen_req).await.unwrap();
        let body = to_bytes(gen_resp.into_body(), usize::MAX).await.unwrap();
        let gen_res: GenerateChallengeResponse = serde_json::from_slice(&body).unwrap();
        let target_id = gen_res.challenges[0].id.clone();

        // Answer challenge
        let ans_req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/challenges/answer")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "challenge_id": target_id,
                    "response": "Detailed resolution to address the detected contradiction in rules."
                })
                .to_string(),
            ))
            .unwrap();

        let ans_resp = app.oneshot(ans_req).await.unwrap();
        assert_eq!(ans_resp.status(), StatusCode::OK);

        let body = to_bytes(ans_resp.into_body(), usize::MAX).await.unwrap();
        let ans_res: AnswerChallengeResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(ans_res.challenge_id, target_id);
        assert!(ans_res.verified);
        assert!(ans_res.points_awarded > 0);
        assert!(ans_res.trust_points > 0);
    }

    #[tokio::test]
    async fn test_list_and_stats_endpoints() {
        let state = ChallengeState::in_memory();
        let app = router(state.clone());

        // 1. Generate challenges
        let gen_req = Request::builder()
            .method("POST")
            .uri("/v1/maloca/challenges/generate")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "session_id": "session_list_test"
                })
                .to_string(),
            ))
            .unwrap();
        let _ = app.clone().oneshot(gen_req).await.unwrap();

        // 2. List challenges
        let list_req = Request::builder()
            .method("GET")
            .uri("/v1/maloca/challenges/list?session_id=session_list_test")
            .body(Body::empty())
            .unwrap();

        let list_resp = app.clone().oneshot(list_req).await.unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);

        let body = to_bytes(list_resp.into_body(), usize::MAX).await.unwrap();
        let list_res: ListChallengesResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(list_res.total, 5);

        // 3. Stats endpoint
        let stats_req = Request::builder()
            .method("GET")
            .uri("/v1/maloca/challenges/stats")
            .body(Body::empty())
            .unwrap();

        let stats_resp = app.oneshot(stats_req).await.unwrap();
        assert_eq!(stats_resp.status(), StatusCode::OK);

        let body = to_bytes(stats_resp.into_body(), usize::MAX).await.unwrap();
        let stats_res: ChallengeStatsResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(stats_res.total_challenges, 5);
        assert_eq!(stats_res.x2_target_points, 10);
    }
}
