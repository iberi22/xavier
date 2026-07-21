//! Provider management handlers for hot-switching LLM providers.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::Response,
};
use serde::Deserialize;
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::agents::provider::router::{ProviderKind, AutoStrategy};

#[derive(Deserialize)]
pub struct ProviderSetPayload {
    pub provider: String,
}

#[derive(Deserialize)]
pub struct ProviderAutoPayload {
    pub strategy: String,
}

/// Provider status handler.
pub async fn provider_status_handler(State(state): State<CliState>) -> Response {
    let (active_mode, current_provider, fallback_chain, history) = {
        let router = state.provider_router.read().await;
        (
            router.active_mode().clone(),
            router.current_provider().as_str().to_string(),
            router.fallback_chain().to_vec(),
            router.history().to_vec(),
        )
    };

    let mode = match xavier::server::alerts::SYSTEM_ALERTS.get_mode() {
        xavier::server::alerts::OperationalMode::LocalHealthy => "local",
        xavier::server::alerts::OperationalMode::LocalDegraded => "degraded",
        xavier::server::alerts::OperationalMode::CloudFallback => "cloud",
        xavier::server::alerts::OperationalMode::Disabled => "disabled",
    };
    let local_reachable = xavier::agents::provider::router::ProviderRouter::is_ollama_reachable().await;

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "active": active_mode,
            "current": current_provider,
            "fallback_chain": fallback_chain,
            "history": history,
            "mode": mode,
            "local_reachable": local_reachable,
        }),
    )
}

/// Provider list handler.
pub async fn provider_list_handler() -> Response {
    let providers = ProviderKind::all();
    let strategies = vec![
        AutoStrategy::LowestLatency.as_str(),
        AutoStrategy::LowestCost.as_str(),
        AutoStrategy::BestQuality.as_str(),
        AutoStrategy::DeterministicOnly.as_str(),
    ];

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "providers": providers.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            "strategies": strategies,
        }),
    )
}

/// Provider set handler.
pub async fn provider_set_handler(
    State(state): State<CliState>,
    Json(payload): Json<ProviderSetPayload>,
) -> Response {
    let mut router = state.provider_router.write().await;
    if let Some(kind) = ProviderKind::from_str(&payload.provider) {
        match router.switch_to(kind) {
            Ok(_) => json_response(
                StatusCode::OK,
                serde_json::json!({
                    "status": "ok",
                    "message": format!("Switched to provider: {}", kind.as_str())
                }),
            ),
            Err(e) => json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": e.to_string() }),
            ),
        }
    } else {
        json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": format!("Unknown provider: {}", payload.provider) }),
        )
    }
}

/// Provider auto handler.
pub async fn provider_auto_handler(
    State(state): State<CliState>,
    Json(payload): Json<ProviderAutoPayload>,
) -> Response {
    let mut router = state.provider_router.write().await;
    if let Some(strategy) = AutoStrategy::from_str(&payload.strategy) {
        router.set_auto_strategy(strategy);
        json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "message": format!("Set auto strategy to: {}", strategy.as_str())
            }),
        )
    } else {
        json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": format!("Unknown strategy: {}", payload.strategy) }),
        )
    }
}
