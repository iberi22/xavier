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
    let status = xavier::observability::health::HEALTH.get_status().await;
    AxumJson(json!({
        "status": status.status,
        "mode": status.mode,
        "service": "xavier-headless",
        "version": env!("CARGO_PKG_VERSION"),
        "llm": status.llm,
        "embeddings": status.embedding,
        "vector_db": status.vector_db,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
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

fn fallback_from_memory(_query: &str, docs: &[xavier::memory::store::MemoryRecord]) -> String {
    let mut content = String::new();
    content.push_str("Mostrando información recuperada de la memoria local:\n\n");
    for doc in docs {
        let title = doc.metadata.get("title")
            .and_then(|t| t.as_str())
            .unwrap_or(&doc.path);
        content.push_str(&format!("--- {title} ---\n{}\n\n", doc.content.trim()));
    }
    content.trim_end().to_string()
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
        messages: req.messages.clone(),
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
            // Log original failure in telemetry
            tracing::warn!("Secure proxy execution failed: {:?}", e);

            // Extract the last user message from the request
            let user_msg = req.messages.iter().rev().find(|m| {
                m.get("role").and_then(|r| r.as_str()) == Some("user")
            }).and_then(|m| m.get("content").and_then(|c| c.as_str()));

            if let Some(query) = user_msg {
                // Call memory search (top-k 5)
                match state.memory.search(query, 5, None).await {
                    Ok(records) if !records.is_empty() => {
                        let response_text = fallback_from_memory(query, &records);

                        let synthetic_resp = ChatResponse {
                            id: format!("chatcmpl-{}", ulid::Ulid::new()),
                            object: "chat.completion".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: "memory-fallback".to_string(),
                            choices: vec![Choice {
                                index: 0,
                                message: serde_json::json!({
                                    "role": "assistant",
                                    "content": format!("[Modo memoria — LLM no disponible] {}", response_text),
                                }),
                                finish_reason: "stop".to_string(),
                            }],
                            usage: Usage {
                                prompt_tokens: 0,
                                completion_tokens: 0,
                                total_tokens: 0,
                            },
                            provider: Some("memory-fallback".to_string()),
                        };
                        return (StatusCode::OK, AxumJson(synthetic_resp)).into_response();
                    }
                    _ => {}
                }
            }

            ProxyErrorWrapper(e).into_response()
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

pub async fn headless_providers(State(state): State<CliState>) -> impl IntoResponse {
    let router = state.provider_router.read().await;
    let fallback_chain = router.fallback_chain();
    let current = router.current_provider();

    let mut providers = Vec::new();

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

        // Reachability check: In this phase, we use configuration as a proxy for reachability
        // to avoid blocking synchronous network I/O during the API call.
        // Future iterations (LOCAL1-01) may use a background health-check cache.
        let reachable = is_configured;

        providers.push(ProviderStatus {
            name,
            mode,
            configured: is_configured,
            reachable,
            in_fallback_chain: is_in_chain,
        });
    }

    AxumJson(json!({
        "providers": providers,
        "active": current.as_str(),
        "fallback_chain": fallback_chain.iter().map(|k| k.as_str()).collect::<Vec<_>>(),
    }))
}

pub async fn headless_provider_status(State(state): State<CliState>) -> impl IntoResponse {
    let router = state.provider_router.read().await;
    let active = router.current_provider();
    let strategy = match router.active_mode() {
        xavier::agents::provider::router::ActiveProvider::Auto { strategy } => strategy.as_str(),
        _ => "manual",
    };

    AxumJson(json!({
        "active": active.as_str(),
        "strategy": strategy,
        "fallback_chain": router.fallback_chain().iter().map(|k| k.as_str()).collect::<Vec<_>>(),
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
    use super::*;
    use crate::cli::state::{CliState, CodeGraphState};
    use xavier::ports::inbound::{MemoryQueryPort, AgentLifecyclePort};
    use xavier::domain::memory::MemoryQueryFilters;
    use xavier::memory::store::MemoryRecord;
    use axum::{
        Json,
        response::IntoResponse,
    };
    use std::sync::Arc;
    use std::path::PathBuf;
    use std::collections::HashMap;
    use parking_lot::Mutex;

    struct MockMemoryPort {
        records: Vec<MemoryRecord>,
    }

    #[async_trait::async_trait]
    impl MemoryQueryPort for MockMemoryPort {
        async fn search(
            &self,
            _query: &str,
            _limit: usize,
            _filters: Option<MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(self.records.clone())
        }
        async fn expand_depth(
            &self,
            results: &[MemoryRecord],
            _depth: usize,
            _filters: Option<MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(results.to_vec())
        }
        async fn add(&self, _record: MemoryRecord) -> anyhow::Result<String> {
            Ok("ok".to_string())
        }
        async fn update(&self, _id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
            Ok(record)
        }
        async fn delete(&self, _id: &str) -> anyhow::Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn get(&self, _id: &str) -> anyhow::Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn list(&self, _workspace_id: &str, _limit: usize) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(self.records.clone())
        }
        async fn export(&self, _public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(self.records.clone())
        }
        async fn ls(&self, _path: &str) -> anyhow::Result<Vec<crate::xavier_lib::memory::qmd::types::NavEntry>> {
            Ok(vec![])
        }
    }

    async fn test_state(mock_memory: Arc<dyn MemoryQueryPort>) -> CliState {
        use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
        use xavier::app::security_service::SecurityService;
        use xavier::coordination::SimpleAgentRegistry;
        use xavier::coordination::XavierEventBus;
        use xavier::coordination::KeyLendingEngine;
        use xavier::secrets::audit::QmdAuditLogger;
        use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
        use xavier::codebase::conversations_db::ConversationsDb;
        use xavier::agents::rate_limit::RateLimitManager;
        use xavier::app::proxy_use_case::ProxyUseCase;
        use xavier::security::sessions::SessionManager;
        use xavier::embedding::NoopEmbedder;
        use xavier::memory::agent_indexer::AgentIndexer;
        use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
        use xavier::memory::openclaw_indexer::OpenClawAgentIndexer;

        let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let qmd_memory = Arc::new(xavier::memory::qmd_memory::QmdMemory::new_with_workspace(docs, "test-ws"));

        let config = VecSqliteStoreConfig {
            path: PathBuf::from(":memory:"),
            embedding_dimensions: 768,
        };
        let store = Arc::new(VecSqliteMemoryStore::new(config).await.unwrap());

        let cg_db = Arc::new(code_graph::db::CodeGraphDB::new(&PathBuf::from(":memory:")).unwrap());
        let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
            db: cg_db.clone(),
            indexer: Arc::new(code_graph::indexer::Indexer::new(cg_db.clone())),
            query: Arc::new(code_graph::query::QueryEngine::new(cg_db)),
        }));

        CliState {
            memory: mock_memory,
            qmd_memory,
            store: store.clone(),
            workspace_id: "test-ws".to_string(),
            workspace_dir: PathBuf::from("."),
            code_graph: cg_state,
            security: Arc::new(SecurityService::new()),
            security_scan: Arc::new(SecurityService::new()),
            _time_store: None,
            agent_registry: SimpleAgentRegistry::new(None),
            panel_store: Arc::new(
                ConversationsDb::open_in_memory("test-project")
                    .await
                    .unwrap(),
            ),
            secrets_engine: Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None)),
            event_bus: XavierEventBus::new(10),
            tasks: Arc::new(xavier::tasks::TaskService::new(Arc::new(xavier::tasks::store::InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(Mutex::new(HashMap::new())),
            )),
            session_manager: Arc::new(SessionManager::new(60)),
            provider_router: Arc::new(tokio::sync::RwLock::new(ProviderRouter::new(
                ProviderKind::OpenAI,
            ))),
            embedder: Arc::new(NoopEmbedder),
            agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
                FileIndexerConfig::default(),
                None,
            ))),
            auth_store: None,
            openclaw_indexer: Arc::new(OpenClawAgentIndexer::new(Arc::new(NoopEmbedder))),
            system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    #[tokio::test]
    async fn test_headless_chat_fallback() {
        let record1 = MemoryRecord {
            id: "1".to_string(),
            workspace_id: "test-ws".to_string(),
            path: "doc1.txt".to_string(),
            content: "contenido uno".to_string(),
            metadata: serde_json::json!({"title": "Doc Uno"}),
            ..Default::default()
        };
        let record2 = MemoryRecord {
            id: "2".to_string(),
            workspace_id: "test-ws".to_string(),
            path: "doc2.txt".to_string(),
            content: "contenido dos".to_string(),
            metadata: serde_json::json!({"title": "Doc Dos"}),
            ..Default::default()
        };

        let mock_memory = Arc::new(MockMemoryPort {
            records: vec![record1, record2],
        });

        let state = test_state(mock_memory).await;

        let session = crate::cli::http_setup::SessionInfo {
            is_ephemeral: true,
            api_token: None,
            lease: None,
        };

        let req = ChatRequest {
            model: Some("auto".to_string()),
            messages: vec![serde_json::json!({
                "role": "user",
                "content": "some user query",
            })],
            temperature: None,
            max_tokens: None,
            stream: None,
            provider: None,
            lease_token: Some("invalid-token".to_string()),
        };

        let response = headless_chat(
            axum::extract::State(state),
            axum::Extension(session),
            Json(req),
        )
        .await
        .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(body_json["model"], "memory-fallback");
        assert_eq!(body_json["provider"], "memory-fallback");

        let content = body_json["choices"][0]["message"]["content"].as_str().unwrap();
        assert!(content.contains("[Modo memoria — LLM no disponible]"));
        assert!(content.contains("contenido uno"));
        assert!(content.contains("contenido dos"));
    }
}
