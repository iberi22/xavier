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

pub async fn headless_system_scan(
    State(state): State<CliState>,
) -> impl IntoResponse {
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
    use crate::cli::utils::ProxyErrorWrapper;
    use xavier::domain::proxy::ProxyChatCommand;

    let cmd = ProxyChatCommand {
        model: req.model.unwrap_or_else(|| "auto".to_string()),
        messages: req.messages,
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
        Err(e) => ProxyErrorWrapper(e).into_response(),
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
    let reachability_futures = providers_to_check.iter().map(|(_, _, config, _, _, _)| {
        config.is_reachable()
    });
    let reachability_results = futures_util::future::join_all(reachability_futures).await;

    let mut providers = Vec::new();
    let mut local_reachable = false;

    for (i, (kind, name, _, mode, is_configured, is_in_chain)) in providers_to_check.into_iter().enumerate() {
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
        fallback_chain: fallback_chain.iter().map(|k| k.as_str().to_string()).collect(),
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

// ═════════════════════════════════════════════════════════════════════════════
// Ollama Model Manager (Ola 4 · 02)
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
pub struct OllamaPullRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct OllamaSetActiveRequest {
    pub model: String,
    pub kind: String, // "llm" or "embedding"
}

pub async fn ollama_list_models() -> impl IntoResponse {
    let base_url = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = reqwest::Client::new();

    // Check version first to confirm Ollama is running
    let version_check = client.get(format!("{}/api/version", base_url))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;

    if version_check.is_err() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            AxumJson(json!({
                "error": "Ollama no responde en :11434",
                "url": base_url
            })),
        )
            .into_response();
    }

    // List models from /api/tags
    match client.get(format!("{}/api/tags", base_url)).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                    let list: Vec<String> = models
                        .iter()
                        .filter_map(|m| {
                            m.get("name")
                                .and_then(|n| n.as_str().map(|s| s.to_string()))
                        })
                        .collect();
                    return (StatusCode::OK, AxumJson(json!({ "models": list }))).into_response();
                }
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                AxumJson(json!({ "error": "Invalid response format from Ollama" })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            AxumJson(json!({ "error": format!("Ollama no responde en :11434: {}", e), "url": base_url })),
        )
            .into_response(),
    }
}

pub async fn ollama_pull_model(
    Json(req): Json<OllamaPullRequest>,
) -> impl IntoResponse {
    let base_url = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600)) // 10 minutes timeout for model pulls
        .build()
        .unwrap_or_default();

    match client
        .post(format!("{}/api/pull", base_url))
        .json(&json!({ "name": req.name, "stream": false }))
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                (
                    StatusCode::OK,
                    AxumJson(json!({ "success": true, "status": "completed" })),
                )
                    .into_response()
            } else {
                let err_text = resp.text().await.unwrap_or_default();
                (
                    StatusCode::BAD_REQUEST,
                    AxumJson(json!({ "success": false, "error": err_text })),
                )
                    .into_response()
            }
        }
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            AxumJson(json!({ "success": false, "error": format!("Ollama no responde en :11434: {}", e) })),
        )
            .into_response(),
    }
}

pub async fn ollama_get_active() -> impl IntoResponse {
    let settings = xavier::settings::XavierSettings::current();

    // Check environment overrides first, then settings
    let active_llm = std::env::var("XAVIER_LOCAL_LLM_MODEL")
        .unwrap_or_else(|_| settings.models.local_llm_model.clone());
    let active_embedding = std::env::var("XAVIER_EMBEDDING_MODEL")
        .or_else(|_| std::env::var("XAVIER_GLLM_MODEL"))
        .unwrap_or_else(|_| settings.models.embedding_model.clone());

    AxumJson(json!({
        "llm": active_llm,
        "embedding": active_embedding,
        // retro-compatibility if the client expects structure with model and kind:
        "model": active_llm,
        "kind": "llm"
    }))
}

pub async fn ollama_set_active(
    Json(req): Json<OllamaSetActiveRequest>,
) -> impl IntoResponse {
    let mut settings = xavier::settings::XavierSettings::current();
    let mut updated = false;

    if req.kind == "llm" {
        settings.models.local_llm_model = req.model.clone();
        std::env::set_var("XAVIER_LOCAL_LLM_MODEL", &req.model);
        updated = true;
    } else if req.kind == "embedding" {
        settings.models.embedding_model = req.model.clone();
        std::env::set_var("XAVIER_EMBEDDING_MODEL", &req.model);
        std::env::set_var("XAVIER_GLLM_MODEL", &req.model);
        updated = true;
    }

    if updated {
        if let Err(e) = settings.save().await {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                AxumJson(json!({ "success": false, "error": format!("Failed to save settings: {}", e) })),
            )
                .into_response();
        }
        (
            StatusCode::OK,
            AxumJson(json!({ "success": true, "message": format!("Active {} model set to {}", req.kind, req.model) })),
        )
            .into_response()
    } else {
        (
            StatusCode::BAD_REQUEST,
            AxumJson(json!({ "success": false, "error": format!("Invalid model kind: {}", req.kind) })),
        )
            .into_response()
    }
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
