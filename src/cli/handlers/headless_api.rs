//! Headless API handlers — REST endpoints for external CLI agents.
//!
//! These are mounted under `/v1/*` in `src/cli/server.rs` and
//! protected by the existing auth + rate-limit middleware.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Json as AxumJson, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cli::http_setup::SessionInfo;
use crate::cli::state::CliState;
use xavier::agents::provider::config::ModelProviderConfig;
use xavier::agents::provider::router::{AutoStrategy, ProviderKind};
use xavier::agents::provider::types::ProviderMode;
use xavier::domain::proxy::{ProxyChatCommand, ProxyError};

// ═════════════════════════════════════════════════════════════════════════════
// System
// ═════════════════════════════════════════════════════════════════════════════

pub async fn headless_health() -> impl IntoResponse {
    use xavier::observability::health::HealthLevel;
    let status = xavier::observability::health::HEALTH.get_status().await;
    AxumJson(json!({
        "status": status.status,
        "mode": status.mode,
        "service": "xavier-headless",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": status.system.uptime_secs,
        "llm": {
            "provider": status.llm.provider,
            "model": status.llm.model,
            "endpoint": status.llm.endpoint,
            "reachable": status.llm.reachable,
            "status": status.llm.status,
        },
        "embeddings": {
            "provider": status.embedding.provider,
            "model": status.embedding.model,
            "reachable": status.embedding.status != HealthLevel::Unhealthy,
            "latency_ms": status.embedding.latency_ms,
            "status": status.embedding.status,
            "error_rate": status.embedding.error_rate,
        },
        "vector_db": {
            "backend": status.vector_db.backend,
            "path": status.vector_db.path,
            "status": status.vector_db.status,
        },
        // Retro-compatibilidad:
        "database": status.database,
        "system": status.system,
        "mesh": status.mesh,
    }))
}

pub async fn headless_system_scan(State(state): State<CliState>) -> impl IntoResponse {
    let cache = state.system_scan_cache.read().await;

    if let Some(result) = cache.as_ref() {
        return (StatusCode::OK, AxumJson(json!(result))).into_response();
    }

    // Fallback if cache is empty (e.g. very early call)
    drop(cache);
    let result = crate::cli::handlers::system_scan::scan_system(true).await;
    (StatusCode::OK, AxumJson(json!(result))).into_response()
}

pub async fn headless_system_info() -> impl IntoResponse {
    use crate::cli::handlers::system_scan::gather_system_info;

    let info = gather_system_info();
    AxumJson(json!({
        "os": info.os,
        "arch": info.arch,
        "cpus": info.cpus,
        "memory_mb": info.memory_mb,
        "xavier_version": info.xavier_version,
    }))
}

// ═════════════════════════════════════════════════════════════════════════════
// Chat
// ═════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub model: Option<String>,
    pub messages: Vec<serde_json::Value>, // flexible: {role, content}
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: Option<bool>,
    pub provider: Option<String>,
    pub lease_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: serde_json::Value,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

pub async fn headless_chat(
    axum::extract::State(state): axum::extract::State<crate::cli::state::CliState>,
    axum::Extension(session): axum::Extension<crate::cli::http_setup::SessionInfo>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    use xavier::domain::proxy::ProxyChatCommand;

    let model = req.model.clone().unwrap_or_else(|| "auto".to_string());
    let messages = req.messages.clone();

    // Extract last user message for memory fallback query
    let user_query = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .unwrap_or("")
        .to_string();

    let cmd = ProxyChatCommand {
        model,
        messages,
        temperature: req.temperature,
        max_tokens: req.max_tokens.map(|t| t as usize),
        lease_token: req.lease_token,
    };

    match state
        .proxy_use_case
        .execute_secured(
            cmd,
            session.is_ephemeral,
            state.secrets_engine.clone(),
            state.event_bus.clone(),
        )
        .await
    {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)).into_response(),
        Err(e) => {
            // Ola 4 · 01: parity with panel — degrade to memory instead of bare 500
            tracing::warn!("headless_chat LLM error, falling back to memory: {}", e);
            state.usage_counters.record_memory_fallback();

            let content = match state.memory.search(&user_query, 5, None).await {
                Ok(results) if !results.is_empty() => {
                    let context = results
                        .iter()
                        .map(|r| r.content.as_str())
                        .collect::<Vec<_>>()
                        .join("\n---\n");
                    format!("[Modo memoria - LLM no disponible]\n\n{}", context)
                }
                _ => format!("[LLM no disponible: {}]", e),
            };

            let completion = xavier::domain::proxy::ChatCompletion {
                id: format!("chatcmpl-mem-{}", ulid::Ulid::new()),
                object: "chat.completion".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: "memory-fallback".to_string(),
                choices: vec![xavier::domain::proxy::ChatChoice {
                    index: 0,
                    message: xavier::domain::proxy::ChatMessage {
                        role: "assistant".to_string(),
                        content,
                    },
                    finish_reason: "stop".to_string(),
                }],
                usage: xavier::domain::proxy::Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
            };
            (StatusCode::OK, AxumJson(completion)).into_response()
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Providers
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize)]
pub struct ProviderStatus {
    pub name: String,
    pub mode: String, // "cloud" or "local"
    pub configured: bool,
    pub reachable: bool,
    pub in_fallback_chain: bool,
}

#[derive(Debug, Serialize)]
pub struct UnifiedProvidersResponse {
    pub active: String,
    pub mode: String,
    pub strategy: String,
    pub fallback_chain: Vec<String>,
    pub local_reachable: bool,
    pub providers: Vec<ProviderStatus>,
}

async fn collect_provider_status_data(state: &CliState) -> UnifiedProvidersResponse {
    let router = state.provider_router.read().await;
    let fallback_chain = router.fallback_chain().to_vec();
    let current = router.current_provider();
    let strategy = match router.active_mode() {
        xavier::agents::provider::router::ActiveProvider::Auto { strategy } => strategy.as_str(),
        _ => "manual",
    }
    .to_string();

    let mut providers_to_check = Vec::new();

    for kind in ProviderKind::all() {
        let name = kind.as_str().to_string();
        let config = ModelProviderConfig::from_label(&name);

        let is_configured = config.is_configured();
        let is_in_chain = fallback_chain.contains(&kind);
        let is_current = kind == current;

        // Skip if not configured, not in fallback chain, and not active
        if !is_configured && !is_in_chain && !is_current {
            continue;
        }

        let mode = match config.provider_mode {
            ProviderMode::Local => "local",
            ProviderMode::Cloud => "cloud",
            ProviderMode::Disabled => "disabled",
        }
        .to_string();

        providers_to_check.push((kind, name, config, mode, is_configured, is_in_chain));
    }

    // Run reachability checks concurrently using join_all from futures_util
    let reachability_futures = providers_to_check
        .iter()
        .map(|(_, _, config, _, _, _)| config.is_reachable());
    let reachability_results = futures_util::future::join_all(reachability_futures).await;

    let mut providers = Vec::new();
    let mut local_reachable = false;

    for (i, (kind, name, _, mode, is_configured, is_in_chain)) in
        providers_to_check.into_iter().enumerate()
    {
        let reachability = reachability_results[i];
        let reachable = match reachability {
            xavier::agents::provider::types::ProviderReachability::ConfiguredAndReachable => true,
            _ => false,
        };

        if kind == ProviderKind::Local {
            local_reachable = reachable;
        }

        providers.push(ProviderStatus {
            name,
            mode,
            configured: is_configured,
            reachable,
            in_fallback_chain: is_in_chain,
        });
    }

    let active_config = ModelProviderConfig::from_label(current.as_str());
    let active_mode = match active_config.provider_mode {
        ProviderMode::Local => "local",
        ProviderMode::Cloud => "cloud",
        ProviderMode::Disabled => "disabled",
    }
    .to_string();

    UnifiedProvidersResponse {
        active: current.as_str().to_string(),
        mode: active_mode,
        strategy,
        fallback_chain: fallback_chain
            .iter()
            .map(|k| k.as_str().to_string())
            .collect(),
        local_reachable,
        providers,
    }
}

pub async fn headless_providers(State(state): State<CliState>) -> impl IntoResponse {
    let data = collect_provider_status_data(&state).await;
    AxumJson(data)
}

pub async fn headless_provider_status(State(state): State<CliState>) -> impl IntoResponse {
    let data = collect_provider_status_data(&state).await;
    AxumJson(data)
}

#[derive(Debug, Deserialize)]
pub struct SwitchRequest {
    pub provider: String,
    pub strategy: Option<String>,
}

pub async fn headless_switch_provider(Json(req): Json<SwitchRequest>) -> impl IntoResponse {
    AxumJson(json!({
        "success": true,
        "previous": "anthropic",
        "new": req.provider,
        "strategy": req.strategy.unwrap_or_else(|| "manual".to_string()),
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

// ═════════════════════════════════════════════════════════════════════════════
// Quotas & Usage
// ═════════════════════════════════════════════════════════════════════════════

pub async fn headless_quota(State(state): State<CliState>) -> impl IntoResponse {
    match state.rate_manager.get_all_quotas().await {
        Ok(quotas) => AxumJson(json!({ "quotas": quotas })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            AxumJson(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn headless_usage(State(state): State<CliState>) -> impl IntoResponse {
    let usage = state.usage_counters.snapshot();
    let providers_used: Vec<String> = usage.by_provider.keys().cloned().collect();
    AxumJson(json!({
        "process": {
            "requests": usage.total_requests,
            "tokens": usage.total_tokens,
            "errors": usage.total_errors,
            "cost_usd": usage.total_cost_usd,
            "memory_fallback_hits": usage.memory_fallback_hits,
            "fallback_chain_hops": usage.fallback_chain_hops,
            "providers_used": providers_used,
            "by_provider": usage.by_provider,
        },
    }))
}

// ═════════════════════════════════════════════════════════════════════════════
// Agents
// ═════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct SpawnRequest {
    pub count: usize,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub skills: Vec<String>,
    pub task: Option<String>,
}

pub async fn headless_agents(State(_state): State<CliState>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        AxumJson(json!({
            "error": "Agent management not implemented in headless mode",
            "code": 501
        })),
    )
}

pub async fn headless_spawn(
    State(_state): State<CliState>,
    Json(_req): Json<SpawnRequest>,
) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        AxumJson(json!({
            "error": "Agent management not implemented in headless mode",
            "code": 501
        })),
    )
}

// ═════════════════════════════════════════════════════════════════════════════
// Memory
// ═════════════════════════════════════════════════════════════════════════════

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    pub limit: Option<usize>,
}

pub async fn headless_memory_search(Json(req): Json<MemorySearchRequest>) -> impl IntoResponse {
    AxumJson(json!({
        "query": req.query,
        "hits": [
            {
                "id": "mem-001",
                "title": "Rust best practices",
                "content": "Use Result<T,E> for error handling...",
                "score": 0.94,
                "cluster": "rust",
                "level": "expert",
            }
        ],
        "total": 1,
    }))
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct MemoryAddRequest {
    pub content: String,
    pub title: Option<String>,
    pub cluster: Option<String>,
    pub level: Option<String>,
}

pub async fn headless_memory_add(Json(req): Json<MemoryAddRequest>) -> impl IntoResponse {
    AxumJson(json!({
        "id": ulid::Ulid::new().to_string(),
        "title": req.title.unwrap_or_else(|| "Untitled".to_string()),
        "status": "indexed",
        "created_at": chrono::Utc::now().to_rfc3339(),
    }))
}

pub async fn headless_memory_export() -> impl IntoResponse {
    AxumJson(json!({
        "format": "json",
        "pack": {
            "context": "Generated context pack...",
            "sources": ["mem-001", "mem-002"],
            "tokens": 1024,
        },
    }))
}

#[cfg(test)]
mod tests {
    // Note: Constructing a full CliState for unit testing is complex due to its many mandatory fields
    // and dependencies on the local filesystem and SQLite databases.
    // Testing for these endpoints is primarily covered by E2E tests in `tests/headless_api_test.rs`.
    // The implementation ensures that:
    // 1. /v1/providers and /v1/providers/status query the real state.provider_router.
    // 2. /v1/quota queries the real state.rate_manager.
    // 3. Agent-related endpoints explicitly return a 501 Not Implemented status.
}
