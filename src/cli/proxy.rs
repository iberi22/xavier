//! CLI commands for proxy management
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::cli::http_setup::SessionInfo;
use crate::cli::state::CliState;
use crate::cli::utils::ProxyErrorWrapper;
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use tracing::warn;
use xavier::domain::proxy::{GenericProxyRequest, ProxyChatCommand};

#[derive(Debug, Deserialize)]
pub struct ProxyChatRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<usize>,
    pub lease_token: Option<String>,
}

impl From<ProxyChatRequest> for ProxyChatCommand {
    fn from(req: ProxyChatRequest) -> Self {
        Self {
            model: req.model,
            messages: req.messages,
            temperature: req.temperature,
            max_tokens: req.max_tokens,
            lease_token: req.lease_token,
        }
    }
}

pub async fn chat_proxy(
    State(state): State<CliState>,
    axum::Extension(session): axum::Extension<SessionInfo>,
    Json(req): Json<ProxyChatRequest>,
) -> Response {
    match state
        .proxy_use_case
        .execute_secured(
            req.into(),
            session.is_ephemeral,
            state.secrets_engine.clone(),
            state.event_bus.clone(),
        )
        .await
    {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => ProxyErrorWrapper(e).into_response(),
    }
}

pub async fn chat_batch_proxy(
    State(state): State<CliState>,
    axum::Extension(session): axum::Extension<SessionInfo>,
    Json(requests): Json<Vec<ProxyChatRequest>>,
) -> Response {
    let mut results = vec![serde_json::json!(null); requests.len()];
    let mut join_set = tokio::task::JoinSet::new();

    for (idx, req) in requests.into_iter().enumerate() {
        let use_case = state.proxy_use_case.clone();
        let secrets = state.secrets_engine.clone();
        let events = state.event_bus.clone();
        join_set.spawn(async move {
            let res = use_case
                .execute_secured(req.into(), session.is_ephemeral, secrets, events)
                .await;
            (idx, res)
        });
    }

    while let Some(res) = join_set.join_next().await {
        match res {
            Ok((idx, Ok(val))) => {
                results[idx] = serde_json::to_value(val).unwrap_or(serde_json::json!(null));
            }
            Ok((idx, Err(e))) => {
                results[idx] = serde_json::json!({
                    "error": e.to_string(),
                    "status": match e {
                        xavier::domain::proxy::ProxyError::RateLimited => 429,
                        _ => 500,
                    }
                });
            }
            Err(e) => {
                warn!("Batch task failed: {e}");
            }
        }
    }

    (StatusCode::OK, Json(results)).into_response()
}

pub async fn revoke_lease_by_path(
    State(state): State<CliState>,
    axum::Extension(_session): axum::Extension<SessionInfo>,
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Response {
    match state
        .secrets_engine
        .revoke(&token, "Proxy Auto-Revoke API")
        .await
    {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "status": "revoked" })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn generic_proxy(
    State(state): State<CliState>,
    axum::Extension(session): axum::Extension<SessionInfo>,
    Json(req): Json<GenericProxyRequest>,
) -> Response {
    match state
        .proxy_use_case
        .execute_generic(req, state.secrets_engine.clone())
        .await
    {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => ProxyErrorWrapper(e).into_response(),
    }
}
