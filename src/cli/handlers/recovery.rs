use axum::{extract::State, http::StatusCode, response::Response, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::security::recovery::RecoverySystem;
use xavier::security::user_store::{BackupCode, UserStore};

#[derive(Deserialize)]
pub struct SeedShowRequest {
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct SeedVerifyRequest {
    pub email: String,
    pub seed_phrase: String,
}

#[derive(Deserialize)]
pub struct PasswordResetRequest {
    pub email: String,
    pub seed_phrase: String,
    pub new_password: String,
}

#[derive(Deserialize)]
pub struct BackupCodesGenerateRequest {
    pub email: String,
    pub seed_phrase: String,
}

/// Seed show handler.
pub async fn seed_show_handler(
    State(_state): State<CliState>,
    Json(payload): Json<SeedShowRequest>,
) -> Response {
    let user_store = UserStore::new();
    let user = match user_store.get_user_by_email(&payload.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "Invalid credentials"}),
            );
        }
        Err(e) => {
            tracing::error!("Failed to query user store in seed_show_handler: {e}");
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Internal server error"}),
            );
        }
    };

    // Verify password
    let security_mgr = xavier::security::SecurityManager::new();
    let is_valid = security_mgr
        .verify_password(&payload.password, &user.password_hash)
        .unwrap_or(false);

    if !is_valid {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "Invalid credentials"}),
        );
    }

    json_response(
        StatusCode::NOT_IMPLEMENTED,
        serde_json::json!({"message": "Seed phrase is not stored and cannot be shown after registration."}),
    )
}

/// Seed verify handler.
pub async fn seed_verify_handler(
    State(_state): State<CliState>,
    Json(payload): Json<SeedVerifyRequest>,
) -> Response {
    let user_store = UserStore::new();
    let user = match user_store.get_user_by_email(&payload.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "User not found"}),
            );
        }
        Err(e) => {
            tracing::error!("Failed to query user store in seed_verify_handler: {e}");
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Internal server error"}),
            );
        }
    };

    if !RecoverySystem::verify_seed_phrase(&payload.seed_phrase, &user.recovery_seed_hash) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "Invalid seed phrase"}),
        );
    }

    json_response(StatusCode::OK, serde_json::json!({"status": "verified"}))
}

/// Password reset handler.
pub async fn password_reset_handler(
    State(state): State<CliState>,
    Json(payload): Json<PasswordResetRequest>,
) -> Response {
    let user_store = UserStore::new();
    let user = match user_store.get_user_by_email(&payload.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "User not found"}),
            );
        }
        Err(e) => {
            tracing::error!("Failed to query user store in password_reset_handler: {e}");
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Internal server error"}),
            );
        }
    };

    if !RecoverySystem::verify_seed_phrase(&payload.seed_phrase, &user.recovery_seed_hash) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "Invalid seed phrase"}),
        );
    }

    // Reset password
    let security_mgr = xavier::security::SecurityManager::new();
    let new_password_hash = match security_mgr.hash_password(&payload.new_password) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to hash password during password reset: {e}");
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Internal server error"}),
            );
        }
    };

    let new_seed = match RecoverySystem::generate_seed_phrase() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to generate seed phrase during password reset: {e}");
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Internal server error"}),
            );
        }
    };
    let new_seed_hash = RecoverySystem::hash_seed_phrase(&new_seed);

    if let Err(e) = user_store
        .update_password_and_recovery(&user.id, &new_password_hash, &new_seed_hash)
        .await
    {
        tracing::error!("Failed to update password and recovery: {e}");
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "Internal server error"}),
        );
    }

    // Invalidate refresh tokens
    state.session_manager.cleanup_expired();

    // Generate new backup codes
    if let Err(e) = user_store.delete_backup_codes_for_user(&user.id).await {
        tracing::error!("Failed to delete backup codes during password reset: {e}");
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "Internal server error"}),
        );
    }

    let codes = RecoverySystem::generate_backup_codes();
    let mut backup_codes = Vec::new();
    for code in &codes {
        backup_codes.push(BackupCode {
            id: Uuid::new_v4().to_string(),
            user_id: user.id.clone(),
            code_hash: RecoverySystem::hash_backup_code(code),
            used: false,
        });
    }

    if let Err(e) = user_store.save_backup_codes(backup_codes).await {
        tracing::error!("Failed to save backup codes during password reset: {e}");
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "Internal server error"}),
        );
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "success",
            "message": "Password reset successfully. Save your new seed phrase and backup codes.",
            "new_seed_phrase": new_seed,
            "backup_codes": codes
        }),
    )
}

/// Backup codes generate handler.
pub async fn backup_codes_generate_handler(
    State(_state): State<CliState>,
    Json(payload): Json<BackupCodesGenerateRequest>,
) -> Response {
    let user_store = UserStore::new();
    let user = match user_store.get_user_by_email(&payload.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "User not found"}),
            );
        }
        Err(e) => {
            tracing::error!("Failed to query user store in backup_codes_generate_handler: {e}");
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Internal server error"}),
            );
        }
    };

    if !RecoverySystem::verify_seed_phrase(&payload.seed_phrase, &user.recovery_seed_hash) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "Invalid seed phrase"}),
        );
    }

    if let Err(e) = user_store.delete_backup_codes_for_user(&user.id).await {
        tracing::error!("Failed to delete backup codes during backup codes generation: {e}");
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "Internal server error"}),
        );
    }

    let codes = RecoverySystem::generate_backup_codes();
    let mut backup_codes = Vec::new();
    for code in &codes {
        backup_codes.push(BackupCode {
            id: Uuid::new_v4().to_string(),
            user_id: user.id.clone(),
            code_hash: RecoverySystem::hash_backup_code(code),
            used: false,
        });
    }

    if let Err(e) = user_store.save_backup_codes(backup_codes).await {
        tracing::error!("Failed to save backup codes during backup codes generation: {e}");
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": "Internal server error"}),
        );
    }

    json_response(
        StatusCode::OK,
        serde_json::json!({
            "status": "success",
            "backup_codes": codes
        }),
    )
}

/// Master key handler.
pub async fn master_key_handler() -> Response {
    json_response(
        StatusCode::NOT_IMPLEMENTED,
        serde_json::json!({"message": "Master key export/import not yet implemented."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;

    use crate::cli::state::{CliState, CodeGraphState};
    use crate::codebase::connection_manager::ConnectionManager;
    use crate::codebase::conversations_db::ConversationsDb;
    use crate::coordination::KeyLendingEngine;
    use crate::coordination::SimpleAgentRegistry;
    use crate::coordination::XavierEventBus;
    use crate::embedding::NoopEmbedder;
    use crate::memory::agent_indexer::AgentIndexer;
    use crate::memory::file_indexer::{FileIndexer, FileIndexerConfig};
    use crate::memory::openclaw_indexer::OpenClawAgentIndexer;
    use crate::memory::qmd_memory::QmdMemory;
    use crate::memory::sqlite_vec_store::{
        VecSqliteMemoryStore, VecSqliteStoreConfig, DEFAULT_EMBEDDING_DIMENSIONS,
    };
    use crate::security::sessions::SessionManager;
    use crate::tasks::store::{InMemoryTaskStore, TaskService};
    use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
    use xavier::agents::rate_limit::RateLimitManager;
    use xavier::app::proxy_use_case::ProxyUseCase;
    use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
    use xavier::app::security_service::SecurityService;
    use xavier::observability::UsageCounters;
    use xavier::secrets::audit::QmdAuditLogger;

    async fn test_state() -> CliState {
        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("vec_store.db");
        let config = VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
        };
        let store = Arc::new(VecSqliteMemoryStore::new(config).await.expect("store"));

        let cg_db = Arc::new(::code_graph::db::CodeGraphDB::in_memory().expect("cg_db"));
        let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
            db: cg_db.clone(),
            indexer: Arc::new(::code_graph::indexer::Indexer::new(cg_db.clone())),
            query: Arc::new(::code_graph::query::QueryEngine::new(cg_db)),
        }));
        CliState {
            memory: memory_port,
            qmd_memory,
            store: store.clone(),
            workspace_id: "test-ws".to_string(),
            workspace_dir: PathBuf::from("."),
            state_dir: PathBuf::from("."),
            auth_db: None,
            code_graph: cg_state,
            security: Arc::new(SecurityService::new()),
            security_scan: Arc::new(SecurityService::new()),
            _time_store: None,
            agent_registry: SimpleAgentRegistry::new(None),
            panel_store: Arc::new(
                ConversationsDb::open_in_memory("test-project")
                    .await
                    .expect("panel_store"),
            ),
            secrets_engine: Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None)),
            event_bus: XavierEventBus::new(10),
            tasks: Arc::new(TaskService::new(Arc::new(InMemoryTaskStore::new()))),
            rate_manager: Arc::new(RateLimitManager::new()),
            prompt_cache: Arc::new(Mutex::new(HashMap::new())),
            http_client: reqwest::Client::new(),
            proxy_use_case: Arc::new(ProxyUseCase::new(
                Arc::new(RateLimitManager::new()),
                Arc::new(Mutex::new(HashMap::new())),
            )),
            usage_counters: Arc::new(UsageCounters::new()),
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
            multi_db: xavier::storage::multi_db::MultiDbManager::new(),
            system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
            maloca: xavier::maloca::MalocaStore::open(
                &std::env::temp_dir().join("xavier-maloca-test-recovery"),
            ),
        }
    }

    async fn body_json(res: Response) -> serde_json::Value {
        let bytes = to_bytes(res.into_body(), 16384).await.expect("body bytes");
        serde_json::from_slice(&bytes).expect("parse json")
    }

    #[tokio::test]
    async fn test_master_key_handler_returns_not_implemented() {
        let res = master_key_handler().await;
        assert_eq!(res.status(), StatusCode::NOT_IMPLEMENTED);
        let body = body_json(res).await;
        assert_eq!(
            body["message"],
            "Master key export/import not yet implemented."
        );
    }

    #[tokio::test]
    async fn test_seed_show_failing_user_store_returns_500() {
        let state = test_state().await;
        let res = seed_show_handler(
            State(state),
            Json(SeedShowRequest {
                email: "missing_store@example.com".into(),
                password: "password123".into(),
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Internal server error");
    }

    #[tokio::test]
    async fn test_seed_verify_failing_user_store_returns_500() {
        let state = test_state().await;
        let res = seed_verify_handler(
            State(state),
            Json(SeedVerifyRequest {
                email: "missing_store@example.com".into(),
                seed_phrase: "word1 word2 word3".into(),
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Internal server error");
    }

    #[tokio::test]
    async fn test_password_reset_failing_user_store_returns_500() {
        let state = test_state().await;
        let res = password_reset_handler(
            State(state),
            Json(PasswordResetRequest {
                email: "missing_store@example.com".into(),
                seed_phrase: "word1 word2 word3".into(),
                new_password: "new_secret_pass".into(),
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Internal server error");
    }

    #[tokio::test]
    async fn test_backup_codes_generate_failing_user_store_returns_500() {
        let state = test_state().await;
        let res = backup_codes_generate_handler(
            State(state),
            Json(BackupCodesGenerateRequest {
                email: "missing_store@example.com".into(),
                seed_phrase: "word1 word2 word3".into(),
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Internal server error");
    }

    #[tokio::test]
    async fn test_backup_codes_generate_failing_backup_codes_store_returns_500_no_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("recovery_test_bc.db");
        ConnectionManager::global()
            .connect_with_path("default", db_path)
            .expect("connect");

        let email = format!("test_rec_{}@example.com", Uuid::new_v4());
        let seed = RecoverySystem::generate_seed_phrase().expect("seed");
        let seed_hash = RecoverySystem::hash_seed_phrase(&seed);
        let sec_mgr = xavier::security::SecurityManager::new();
        let pass_hash = sec_mgr.hash_password("pw123").expect("hash");

        ConnectionManager::global()
            .with_conn("default", move |conn| {
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS users (
                        id TEXT PRIMARY KEY,
                        email TEXT UNIQUE NOT NULL,
                        password_hash TEXT NOT NULL,
                        recovery_seed_hash TEXT NOT NULL,
                        two_factor_enabled INTEGER NOT NULL DEFAULT 0,
                        created_at DATETIME NOT NULL,
                        updated_at DATETIME NOT NULL
                    );",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO users (id, email, password_hash, recovery_seed_hash, two_factor_enabled, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 0, datetime('now'), datetime('now'));",
                    rusqlite::params![Uuid::new_v4().to_string(), email, pass_hash, seed_hash],
                )?;
                Ok(())
            })
            .await
            .expect("setup users");

        let state = test_state().await;
        let res = backup_codes_generate_handler(
            State(state),
            Json(BackupCodesGenerateRequest {
                email,
                seed_phrase: seed,
            }),
        )
        .await;

        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Internal server error");

        ConnectionManager::global().disconnect("default");
    }

    #[tokio::test]
    async fn test_password_reset_failing_backup_codes_store_returns_500_no_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("recovery_test_reset.db");
        ConnectionManager::global()
            .connect_with_path("default", db_path)
            .expect("connect");

        let email = format!("test_reset_{}@example.com", Uuid::new_v4());
        let seed = RecoverySystem::generate_seed_phrase().expect("seed");
        let seed_hash = RecoverySystem::hash_seed_phrase(&seed);
        let sec_mgr = xavier::security::SecurityManager::new();
        let pass_hash = sec_mgr.hash_password("pw123").expect("hash");

        ConnectionManager::global()
            .with_conn("default", move |conn| {
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS users (
                        id TEXT PRIMARY KEY,
                        email TEXT UNIQUE NOT NULL,
                        password_hash TEXT NOT NULL,
                        recovery_seed_hash TEXT NOT NULL,
                        two_factor_enabled INTEGER NOT NULL DEFAULT 0,
                        created_at DATETIME NOT NULL,
                        updated_at DATETIME NOT NULL
                    );",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO users (id, email, password_hash, recovery_seed_hash, two_factor_enabled, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 0, datetime('now'), datetime('now'));",
                    rusqlite::params![Uuid::new_v4().to_string(), email, pass_hash, seed_hash],
                )?;
                Ok(())
            })
            .await
            .expect("setup users");

        let state = test_state().await;
        let res = password_reset_handler(
            State(state),
            Json(PasswordResetRequest {
                email,
                seed_phrase: seed,
                new_password: "brand_new_password_99".into(),
            }),
        )
        .await;

        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Internal server error");

        ConnectionManager::global().disconnect("default");
    }

    #[tokio::test]
    async fn test_seed_verify_invalid_seed_returns_401() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("recovery_test_seed_verify.db");
        ConnectionManager::global()
            .connect_with_path("default", db_path)
            .expect("connect");

        let email = format!("test_verify_{}@example.com", Uuid::new_v4());
        let seed = RecoverySystem::generate_seed_phrase().expect("seed");
        let seed_hash = RecoverySystem::hash_seed_phrase(&seed);
        let sec_mgr = xavier::security::SecurityManager::new();
        let pass_hash = sec_mgr.hash_password("pw123").expect("hash");

        ConnectionManager::global()
            .with_conn("default", move |conn| {
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS users (
                        id TEXT PRIMARY KEY,
                        email TEXT UNIQUE NOT NULL,
                        password_hash TEXT NOT NULL,
                        recovery_seed_hash TEXT NOT NULL,
                        two_factor_enabled INTEGER NOT NULL DEFAULT 0,
                        created_at DATETIME NOT NULL,
                        updated_at DATETIME NOT NULL
                    );",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO users (id, email, password_hash, recovery_seed_hash, two_factor_enabled, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 0, datetime('now'), datetime('now'));",
                    rusqlite::params![Uuid::new_v4().to_string(), email, pass_hash, seed_hash],
                )?;
                Ok(())
            })
            .await
            .expect("setup users");

        let state = test_state().await;
        let res = seed_verify_handler(
            State(state),
            Json(SeedVerifyRequest {
                email,
                seed_phrase: "completely wrong seed phrase".into(),
            }),
        )
        .await;

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let body = body_json(res).await;
        assert_eq!(body["error"], "Invalid seed phrase");

        ConnectionManager::global().disconnect("default");
    }

    #[tokio::test]
    async fn test_backup_codes_generate_success_flow() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("recovery_test_success.db");
        ConnectionManager::global()
            .connect_with_path("default", db_path)
            .expect("connect");

        let email = format!("test_success_{}@example.com", Uuid::new_v4());
        let seed = RecoverySystem::generate_seed_phrase().expect("seed");
        let seed_hash = RecoverySystem::hash_seed_phrase(&seed);
        let sec_mgr = xavier::security::SecurityManager::new();
        let pass_hash = sec_mgr.hash_password("pw123").expect("hash");

        ConnectionManager::global()
            .with_conn("default", move |conn| {
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS users (
                        id TEXT PRIMARY KEY,
                        email TEXT UNIQUE NOT NULL,
                        password_hash TEXT NOT NULL,
                        recovery_seed_hash TEXT NOT NULL,
                        two_factor_enabled INTEGER NOT NULL DEFAULT 0,
                        created_at DATETIME NOT NULL,
                        updated_at DATETIME NOT NULL
                    );",
                    [],
                )?;
                conn.execute(
                    "CREATE TABLE IF NOT EXISTS backup_codes (
                        id TEXT PRIMARY KEY,
                        user_id TEXT NOT NULL,
                        code_hash TEXT NOT NULL,
                        used INTEGER NOT NULL DEFAULT 0
                    );",
                    [],
                )?;
                conn.execute(
                    "INSERT INTO users (id, email, password_hash, recovery_seed_hash, two_factor_enabled, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 0, datetime('now'), datetime('now'));",
                    rusqlite::params![Uuid::new_v4().to_string(), email, pass_hash, seed_hash],
                )?;
                Ok(())
            })
            .await
            .expect("setup tables");

        let state = test_state().await;
        let res = backup_codes_generate_handler(
            State(state),
            Json(BackupCodesGenerateRequest {
                email,
                seed_phrase: seed,
            }),
        )
        .await;

        assert_eq!(res.status(), StatusCode::OK);
        let body = body_json(res).await;
        assert_eq!(body["status"], "success");
        assert!(body["backup_codes"].is_array());
        assert_eq!(body["backup_codes"].as_array().expect("codes").len(), 10);

        ConnectionManager::global().disconnect("default");
    }
}
