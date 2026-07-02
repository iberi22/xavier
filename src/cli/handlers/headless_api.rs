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
use xavier::domain::proxy::{ProxyChatCommand, ProxyError};

// ═════════════════════════════════════════════════════════════════════════════
// System
// ═════════════════════════════════════════════════════════════════════════════

pub async fn headless_health() -> impl IntoResponse {
    AxumJson(json!({
        "status": "ok",
        "service": "xavier-headless",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn headless_system_scan() -> impl IntoResponse {
    use crate::cli::handlers::system_scan::{format_as_json, scan_system};

    let result = scan_system(true).await;
    let formatted = format_as_json(&result);

    match serde_json::from_str::<serde_json::Value>(&formatted) {
        Ok(json) => (StatusCode::OK, AxumJson(json)).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            AxumJson(json!({"error": "Failed to format scan result"})),
        )
            .into_response(),
    }
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
    pub status: String,
    pub latency_ms: Option<u64>,
    pub quota_remaining: Option<f32>,
    pub models: Vec<String>,
    pub strategy: String,
}

pub async fn headless_providers() -> impl IntoResponse {
    let providers = vec![
        ProviderStatus {
            name: "anthropic".to_string(),
            status: "ok".to_string(),
            latency_ms: Some(120),
            quota_remaining: Some(0.85),
            models: vec!["claude-3-5-sonnet".to_string()],
            strategy: "auto:quality".to_string(),
        },
        ProviderStatus {
            name: "openai".to_string(),
            status: "ok".to_string(),
            latency_ms: Some(95),
            quota_remaining: Some(0.92),
            models: vec!["gpt-4o".to_string()],
            strategy: "manual".to_string(),
        },
    ];

    AxumJson(json!({
        "providers": providers,
        "active": "anthropic",
        "fallback_chain": ["openai", "groq"],
    }))
}

pub async fn headless_provider_status() -> impl IntoResponse {
    AxumJson(json!({
        "active": "anthropic",
        "strategy": "auto:quality",
        "fallback_chain": ["openai", "groq", "local"],
    }))
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

pub async fn headless_quota() -> impl IntoResponse {
    AxumJson(json!({
        "quotas": [
            {
                "provider": "anthropic",
                "used_percentage": 0.15,
                "remaining_percentage": 0.85,
                "tokens_used_today": 150_000,
                "tokens_limit_today": 1_000_000,
                "requests_today": 42,
                "status": "healthy",
            },
            {
                "provider": "groq",
                "used_percentage": 0.72,
                "remaining_percentage": 0.28,
                "tokens_used_today": 720_000,
                "tokens_limit_today": 1_000_000,
                "requests_today": 180,
                "cooldown_until": "2026-06-09T22:00:00Z",
                "status": "near-limit",
            },
        ]
    }))
}

pub async fn headless_usage() -> impl IntoResponse {
    AxumJson(json!({
        "today": {
            "requests": 222,
            "tokens": 870_000,
            "cost_usd": 0.45,
            "providers_used": ["anthropic", "groq"],
        },
        "this_week": {
            "requests": 1_540,
            "tokens": 6_200_000,
            "cost_usd": 3.12,
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

pub async fn headless_agents() -> impl IntoResponse {
    AxumJson(json!({
        "agents": [
            {
                "id": "agent-001",
                "status": "running",
                "model": "claude-3-5-sonnet",
                "provider": "anthropic",
                "skills": ["rust", "doc-gen"],
                "task": "Generate API docs",
            }
        ]
    }))
}

pub async fn headless_spawn(Json(req): Json<SpawnRequest>) -> impl IntoResponse {
    let ids: Vec<String> = (0..req.count)
        .map(|i| format!("agent-{:03}", i + 1))
        .collect();

    AxumJson(json!({
        "agents": ids,
        "provider_used": req.provider.unwrap_or_else(|| "auto".to_string()),
        "estimated_cost_usd": 0.05 * req.count as f64,
    }))
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

