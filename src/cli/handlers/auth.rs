//! Authentication API Handlers for Xavier

use axum::{extract::{State, ConnectInfo, Path}, http::{StatusCode, HeaderMap}, response::Response, Json};
use serde::{Deserialize, Serialize};
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::security::auth::{User, UserRole, generate_jwt, validate_jwt, TotpProvider};
use xavier::crypto::password;
use xavier::security::recovery::RecoverySystem;
use chrono::{Utc, Duration};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Deserialize, Clone, Serialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct TotpVerifyRequest {
    pub email: String,
    pub code: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct RecoverRequest {
    pub email: String,
    pub seed_phrase: String,
    pub new_password: String,
}

pub async fn register_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    if auth_store.get_user_by_email(&payload.email).unwrap_or(None).is_some() {
        return json_response(StatusCode::CONFLICT, serde_json::json!({"error": "User already exists"}));
    }

    let password_hash = match password::hash(&payload.password, 0) {
        Ok(h) => h,
        Err(e) => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()})),
    };

    let user = User::new(payload.email.clone(), payload.name, UserRole::User);
    let seed_phrase = RecoverySystem::generate_phrase();

    if let Err(e) = auth_store.create_user(&user, &password_hash) {
        return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()}));
    }

    if let Err(e) = auth_store.set_recovery_phrase(&user.id, &seed_phrase) {
        return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()}));
    }

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let _ = auth_store.log_event(Some(&user.id), "register", ip.as_deref(), ua.as_deref(), None);

    json_response(StatusCode::CREATED, serde_json::json!({
        "status": "ok",
        "user_id": user.id,
        "seed_phrase": seed_phrase
    }))
}

pub async fn login_handler(
    State(state): State<CliState>,
    connect_info: ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .unwrap_or_else(|| {
            connect_info.ip().to_string()
        });

    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    // Rate Limit Check: max 5 failed attempts per 15 minutes (900 seconds) per IP
    let fifteen_mins_ago = Utc::now().timestamp() - 900;
    if let Ok(failed_count) = auth_store.count_failed_logins(&ip, fifteen_mins_ago) {
        if failed_count >= 5 {
            return json_response(
                StatusCode::TOO_MANY_REQUESTS,
                serde_json::json!({"error": "Too many failed login attempts"})
            );
        }
    }

    let (user, hash) = match auth_store.get_user_by_email(&payload.email).unwrap_or(None) {
        Some(u) => u,
        None => {
            let _ = auth_store.log_event(None, "login_failed", Some(&ip), ua.as_deref(), None);
            return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid credentials"}));
        }
    };

    if !password::verify(&payload.password, &hash).unwrap_or(false) {
        let _ = auth_store.log_event(Some(&user.id), "login_failed", Some(&ip), ua.as_deref(), None);
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid credentials"}));
    }

    // Check if TOTP is enabled
    let totp_secret = auth_store.get_totp_secret(&user.id).unwrap_or(None);
    if totp_secret.is_some() {
        return json_response(StatusCode::ACCEPTED, serde_json::json!({
            "status": "mfa_required",
            "email": user.email
        }));
    }

    issue_tokens(&state, &user, Some(&ip), ua.as_deref()).await
}

pub async fn totp_verify_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<TotpVerifyRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let (user, _) = match auth_store.get_user_by_email(&payload.email).unwrap_or(None) {
        Some(u) => u,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid user"})),
    };

    let secret = match auth_store.get_totp_secret(&user.id).unwrap_or(None) {
        Some(s) => s,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "TOTP not enabled"})),
    };

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    let totp = TotpProvider::new("Xavier");
    if !totp.verify_code(&secret, &payload.code) {
        let _ = auth_store.log_event(Some(&user.id), "totp_failed", ip.as_deref(), ua.as_deref(), None);
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid TOTP code"}));
    }

    issue_tokens(&state, &user, ip.as_deref(), ua.as_deref()).await
}

pub async fn refresh_handler(
    State(state): State<CliState>,
    Json(payload): Json<RefreshRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let user_id = match auth_store.verify_refresh_token(&payload.refresh_token).unwrap_or(None) {
        Some(id) => id,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid refresh token"})),
    };

    // Revoke old token
    let _ = auth_store.revoke_refresh_token(&payload.refresh_token);

    let user = auth_store.get_user_by_id(&user_id).unwrap_or(None);
    match user {
        Some(u) => issue_tokens(&state, &u, None, None).await,
        None => json_response(StatusCode::NOT_FOUND, serde_json::json!({"error": "User not found"}))
    }
}

pub async fn recover_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<RecoverRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let (user, _) = match auth_store.get_user_by_email(&payload.email).unwrap_or(None) {
        Some(u) => u,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid user"})),
    };

    let stored_phrase = match auth_store.get_recovery_phrase(&user.id).unwrap_or(None) {
        Some(p) => p,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "Recovery not enabled"})),
    };

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    if stored_phrase != payload.seed_phrase {
        let _ = auth_store.log_event(Some(&user.id), "recovery_failed", ip.as_deref(), ua.as_deref(), None);
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid seed phrase"}));
    }

    let new_hash = password::hash(&payload.new_password, 0).unwrap();
    auth_store.update_password(&user.id, &new_hash).unwrap();

    let _ = auth_store.log_event(Some(&user.id), "recovery_success", ip.as_deref(), ua.as_deref(), None);

    json_response(StatusCode::OK, serde_json::json!({"status": "ok"}))
}

pub async fn totp_setup_handler(
    State(state): State<CliState>,
) -> Response {
    // This should be protected by JWT middleware
    // For now we'll assume we get the user from the state or a placeholder
    // In a real impl, we'd extract it from the token
    json_response(StatusCode::NOT_IMPLEMENTED, serde_json::json!({"error": "Not implemented"}))
}

async fn issue_tokens(state: &CliState, user: &User, ip: Option<&str>, ua: Option<&str>) -> Response {
    let auth_store = state.auth_store().unwrap();
    let secret = std::env::var("XAVIER_JWT_SECRET").unwrap_or_else(|_| "default_secret_change_me".to_string());

    let token: String = match generate_jwt(user, secret.as_bytes()) {
        Ok(t) => t,
        Err(e) => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()})),
    };

    let refresh_token = ulid::Ulid::new().to_string();
    let expires_at = (Utc::now() + Duration::days(7)).timestamp();

    if let Err(e) = auth_store.save_refresh_token(&refresh_token, &user.id, expires_at) {
        return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()}));
    }

    let _ = auth_store.log_event(Some(&user.id), "login_success", ip, ua, None);

    json_response(StatusCode::OK, serde_json::json!({
        "status": "ok",
        "access_token": token,
        "refresh_token": refresh_token,
        "user": user
    }))
}

pub async fn list_sessions_handler(
    State(state): State<CliState>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    match auth_store.get_active_sessions() {
        Ok(sessions) => json_response(StatusCode::OK, serde_json::json!({
            "status": "ok",
            "sessions": sessions
        })),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::Json;
    use std::sync::Arc;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use tokio::sync::RwLock as AsyncRwLock;
    use crate::cli::state::CodeGraphState;
    use xavier::ports::inbound::AgentLifecyclePort;
    use xavier::coordination::SimpleAgentRegistry;
    use xavier::memory::qmd_memory::QmdMemory;
    use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
    use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use xavier::codebase::conversations_db::ConversationsDb;
    use xavier::coordination::KeyLendingEngine;
    use xavier::secrets::audit::QmdAuditLogger;
    use xavier::tasks::store::{TaskService, InMemoryTaskStore};
    use xavier::agents::rate_limit::RateLimitManager;
    use xavier::app::proxy_use_case::ProxyUseCase;
    use xavier::agents::provider::router::{ProviderRouter, ProviderKind};
    use xavier::embedding::NoopEmbedder;
    use xavier::memory::agent_indexer::AgentIndexer;
    use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
    use xavier::security::auth_store::AuthStore;

    async fn create_test_state() -> CliState {
        let auth_store = Arc::new(AuthStore::open(":memory:", [0u8; 32]).unwrap());

        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

        let store_config = VecSqliteStoreConfig {
            path: PathBuf::from(":memory:"),
            embedding_dimensions: 1536,
        };
        let store = Arc::new(VecSqliteMemoryStore::new(store_config).await.unwrap());

        let cg_db = Arc::new(::code_graph::db::CodeGraphDB::in_memory().unwrap());
        let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
            db: cg_db.clone(),
            indexer: Arc::new(::code_graph::indexer::Indexer::new(cg_db.clone())),
            query: Arc::new(::code_graph::query::QueryEngine::new(cg_db)),
        }));

        CliState {
            memory: memory_port,
            qmd_memory,
            store,
            workspace_id: "test-ws".to_string(),
            workspace_dir: std::env::current_dir().unwrap(),
            code_graph: cg_state,
            security: Arc::new(xavier::app::security_service::SecurityService::new()),
            security_scan: Arc::new(xavier::app::security_service::SecurityService::new()),
            _time_store: None,
            agent_registry: SimpleAgentRegistry::new(None) as Arc<dyn AgentLifecyclePort>,
            panel_store: Arc::new(ConversationsDb::open_in_memory("test-project").await.unwrap()),
            secrets_engine: Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None)),
            event_bus: xavier::coordination::XavierEventBus::new(10),
            tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(parking_lot::Mutex::new(HashMap::new())),
            )),
            usage_counters: Arc::new(xavier::observability::UsageCounters::new()),
            session_manager: Arc::new(xavier::security::sessions::SessionManager::new(60)),
            provider_router: Arc::new(tokio::sync::RwLock::new(
                ProviderRouter::new(ProviderKind::Local)
            )),
            embedder: Arc::new(NoopEmbedder),
            agent_indexer: Arc::new(AgentIndexer::new(
                FileIndexer::new(FileIndexerConfig::default(), None)
            )),
            auth_store: Some(auth_store),
            openclaw_indexer: Arc::new(crate::memory::openclaw_indexer::OpenClawAgentIndexer::new(
                Arc::new(NoopEmbedder)
            )),
            system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    #[tokio::test]
    async fn test_login_rate_limiting() {
        let state = create_test_state().await;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 12345));
        let connect_info = ConnectInfo(addr);
        let headers = HeaderMap::new();
        let payload = LoginRequest {
            email: "nonexistent@example.com".to_string(),
            password: "wrong_password".to_string(),
        };

        // First 5 attempts should return 401 Unauthorized because user does not exist
        for _ in 0..5 {
            let res = login_handler(State(state.clone()), connect_info.clone(), headers.clone(), Json(payload.clone())).await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }

        // 6th attempt should return 429 Too Many Requests
        let res = login_handler(State(state.clone()), connect_info.clone(), headers.clone(), Json(payload.clone())).await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}

pub async fn revoke_session_handler(
    State(state): State<CliState>,
    Path(token_id): Path<String>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    match auth_store.revoke_refresh_token(&token_id) {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({
            "status": "ok",
            "message": "Session revoked"
        })),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()})),
    }
}
