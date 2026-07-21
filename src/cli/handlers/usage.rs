// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Usage handlers for tracking and managing provider quotas.

use axum::{
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use tracing::warn;

use crate::cli::config::resolve_http_token;
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::cli::types::*;

pub async fn account_usage_handler(State(state): State<CliState>, headers: HeaderMap) -> Response {
    let expected_token = match resolve_http_token() {
        Ok(token) => token,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "status": "error",
                    "message": format!("Token resolution failed: {e}"),
                }),
            )
        }
    };

    let provided_token = headers
        .get("X-Xavier-Token")
        .and_then(|value| value.to_str().ok());
    if provided_token != Some(expected_token.as_str()) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({
                "status": "error",
                "message": "Unauthorized",
            }),
        );
    }

    let mut provider_quotas = serde_json::Map::new();
    match state.rate_manager.get_all_providers().await {
        Ok(providers) => {
            for p in providers {
                if let Ok(status) = state.rate_manager.get_status(&p).await {
                    if let Ok(val) = serde_json::to_value(&status) {
                        provider_quotas.insert(p, val);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to list providers for quotas: {e}");
        }
    }

    let usage = state.usage_counters.snapshot();
    let by_provider = serde_json::to_value(&usage.by_provider).unwrap_or_default();

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "ok",
            "requests_used": usage.total_requests,
            "total_tokens": usage.total_tokens,
            "total_errors": usage.total_errors,
            "total_cost_usd": usage.total_cost_usd,
            "memory_fallback_hits": usage.memory_fallback_hits,
            "fallback_chain_hops": usage.fallback_chain_hops,
            "by_provider": by_provider,
            "provider_quotas": provider_quotas,
            "optimization": {
                "router_direct_count": 0,
                "semantic_cache_hits": 0,
                "semantic_cache_misses": 0,
            },
        }),
    )
}

pub async fn usage_status_handler(
    State(state): State<CliState>,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    match state.rate_manager.get_status(&provider).await {
        Ok(status) => json_response(
            StatusCode::OK,
            serde_json::to_value(status).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_track_handler(
    State(state): State<CliState>,
    Json(payload): Json<UsageTrackPayload>,
) -> Response {
    match state
        .rate_manager
        .track_request(
            &payload.provider,
            payload.tokens,
            payload.status,
            payload.cost_usd,
            payload.is_cache_hit,
        )
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

#[allow(dead_code)]
pub async fn providers_quota_handler(State(state): State<CliState>) -> Response {
    let mut quotas = Vec::new();
    match state.rate_manager.get_all_providers().await {
        Ok(providers) => {
            for p in providers {
                if let Ok(status) = state.rate_manager.get_status(&p).await {
                    quotas.push(status);
                }
            }
        }
        Err(e) => {
            warn!("Failed to list providers for quotas: {e}");
        }
    }

    // Ensure we always return at least the configured providers even if no usage yet
    let detected_providers = vec!["openai", "anthropic", "gemini", "minimax", "local"];
    for p in detected_providers {
        if !quotas.iter().any(|q| q.provider == p) {
            if let Ok(status) = state.rate_manager.get_status(p).await {
                quotas.push(status);
            }
        }
    }

    json_response(
        StatusCode::OK,
        serde_json::to_value(quotas).unwrap_or_else(|_| serde_json::json!([])),
    )
}

pub async fn usage_summary_handler(
    State(state): State<CliState>,
    AxumPath(provider): AxumPath<String>,
) -> Response {
    match state.rate_manager.get_daily_summary(&provider).await {
        Ok(summary) => json_response(StatusCode::OK, summary),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_update_handler(
    State(state): State<CliState>,
    Json(payload): Json<UsageUpdatePayload>,
) -> Response {
    match state
        .rate_manager
        .update_manual_limit(&payload.provider, payload.percentage)
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

pub async fn usage_cooldown_handler(
    State(state): State<CliState>,
    Json(payload): Json<UsageCooldownPayload>,
) -> Response {
    match state
        .rate_manager
        .report_429(&payload.provider, payload.minutes)
        .await
    {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}
