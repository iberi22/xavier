use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use serde_json::json;
use tower::util::ServiceExt;
use xavier::middleware::require_permission;
use xavier::security::auth::{generate_jwt, validate_jwt, Permission, User, UserRole};

#[test]
fn test_user_roles_initialization() {
    let admin = User::new(
        "admin@swal.dev".to_string(),
        "Super Admin".to_string(),
        UserRole::Admin,
    );
    assert_eq!(admin.role, UserRole::Admin);
    assert!(admin.email.contains("admin"));
    assert!(admin.api_key.starts_with("sk-"));

    let user = User::new(
        "user@swal.dev".to_string(),
        "Regular User".to_string(),
        UserRole::User,
    );
    assert_eq!(user.role, UserRole::User);

    let readonly = User::new(
        "readonly@swal.dev".to_string(),
        "Readonly User".to_string(),
        UserRole::Readonly,
    );
    assert_eq!(readonly.role, UserRole::Readonly);
}

#[test]
fn test_permission_trait_matrix() {
    // 1. Admin Permissions
    let admin = UserRole::Admin;
    assert!(admin.can_view_dashboard());
    assert!(admin.can_search_memory());
    assert!(admin.can_add_memory());
    assert!(admin.can_delete_memory());
    assert!(admin.can_manage_beliefs());
    assert!(admin.can_run_agents());
    assert!(admin.can_view_config());
    assert!(admin.can_edit_config());
    assert!(admin.can_manage_users());

    // 2. Regular User Permissions
    let user = UserRole::User;
    assert!(user.can_view_dashboard());
    assert!(user.can_search_memory());
    assert!(user.can_add_memory());
    assert!(user.can_delete_memory());
    assert!(user.can_manage_beliefs());
    assert!(user.can_run_agents());
    assert!(user.can_view_config());
    assert!(!user.can_edit_config());
    assert!(!user.can_manage_users());

    // 3. Readonly User Permissions
    let readonly = UserRole::Readonly;
    assert!(readonly.can_view_dashboard());
    assert!(readonly.can_search_memory());
    assert!(!readonly.can_add_memory());
    assert!(!readonly.can_delete_memory());
    assert!(!readonly.can_manage_beliefs());
    assert!(!readonly.can_run_agents());
    assert!(readonly.can_view_config());
    assert!(!readonly.can_edit_config());
    assert!(!readonly.can_manage_users());
}

#[test]
fn test_jwt_claims_role_propagation() {
    // Install default crypto provider for jsonwebtoken
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();

    let secret = b"my_super_secret_for_rbac_tests_2026";

    let roles = vec![UserRole::Admin, UserRole::User, UserRole::Readonly];

    for role in roles {
        let user = User::new(
            format!("{:?}@swal.dev", role).to_lowercase(),
            format!("{:?} User", role),
            role,
        );

        let token = generate_jwt(&user, secret).expect("JWT token generation failed");
        let claims = validate_jwt(&token, secret).expect("JWT validation failed");

        assert_eq!(claims.sub, user.id);
        assert_eq!(claims.email, user.email);
        assert_eq!(claims.role, role);
    }
}

#[test]
fn test_jwt_validation_invalid_secret() {
    // Install default crypto provider for jsonwebtoken
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();

    let secret = b"correct_secret_key";
    let wrong_secret = b"wrong_secret_key";

    let user = User::new(
        "test@swal.dev".to_string(),
        "Test User".to_string(),
        UserRole::User,
    );

    let token = generate_jwt(&user, secret).expect("JWT token generation failed");
    let validation_result = validate_jwt(&token, wrong_secret);

    assert!(validation_result.is_err());
}

async fn dummy_delete_handler() -> impl IntoResponse {
    (StatusCode::OK, axum::Json(json!({"status": "deleted"})))
}

#[tokio::test]
async fn test_rbac_http_delete_memory_readonly_forbidden_and_admin_allowed() {
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
    let jwt_secret = "rbac_e2e_secret_key_2026";
    std::env::set_var("XAVIER_JWT_SECRET", jwt_secret);

    let app = Router::new().route(
        "/memory/delete",
        post(dummy_delete_handler).layer(axum::middleware::from_fn(require_permission(|r| {
            r.can_delete_memory()
        }))),
    );

    // 1. Readonly User Request -> expect 403 Forbidden
    let readonly_user = User::new(
        "readonly@swal.dev".to_string(),
        "Readonly User".to_string(),
        UserRole::Readonly,
    );
    let readonly_token =
        generate_jwt(&readonly_user, jwt_secret.as_bytes()).expect("jwt generation failed");

    let claims = validate_jwt(&readonly_token, jwt_secret.as_bytes()).expect("claims");
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/memory/delete")
        .header("content-type", "application/json")
        .body(Body::from(json!({"path": "test/path"}).to_string()))
        .unwrap();
    req.extensions_mut().insert(claims);

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // 2. Admin User Request -> expect 200 OK
    let admin_user = User::new(
        "admin@swal.dev".to_string(),
        "Admin User".to_string(),
        UserRole::Admin,
    );
    let admin_token =
        generate_jwt(&admin_user, jwt_secret.as_bytes()).expect("jwt generation failed");

    let claims = validate_jwt(&admin_token, jwt_secret.as_bytes()).expect("claims");
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/memory/delete")
        .header("content-type", "application/json")
        .body(Body::from(json!({"path": "test/path"}).to_string()))
        .unwrap();
    req.extensions_mut().insert(claims);

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_rbac_mcp_mutative_tools_readonly_blocked() {
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();
    let jwt_secret = "mcp_rbac_e2e_secret";
    std::env::set_var("XAVIER_JWT_SECRET", jwt_secret);
    std::env::set_var("XAVIER_TOKEN", "root_token_123");

    let readonly_user = User::new(
        "readonly@swal.dev".to_string(),
        "Readonly User".to_string(),
        UserRole::Readonly,
    );
    let readonly_token =
        generate_jwt(&readonly_user, jwt_secret.as_bytes()).expect("jwt generation failed");

    let claims = validate_jwt(&readonly_token, jwt_secret.as_bytes()).expect("claims");

    let db_path = std::env::temp_dir().join(format!("rbac_mcp_{}.db", ulid::Ulid::new()));
    let code_db = std::sync::Arc::new(code_graph::db::CodeGraphDB::new(&db_path).unwrap());
    let code_indexer = std::sync::Arc::new(code_graph::indexer::Indexer::new(code_db.clone()));
    let code_query = std::sync::Arc::new(code_graph::query::QueryEngine::new(code_db.clone()));
    let app_state = xavier::AppState {
        workspace_registry: std::sync::Arc::new(xavier::workspace::WorkspaceRegistry::new()),
        indexer: xavier::memory::file_indexer::FileIndexer::new(
            xavier::memory::file_indexer::FileIndexerConfig::default(),
            Some(code_indexer.clone()),
        ),
        agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(
            xavier::memory::file_indexer::FileIndexer::new(
                xavier::memory::file_indexer::FileIndexerConfig::default(),
                Some(code_indexer.clone()),
            ),
        ),
        code_indexer,
        code_query,
        code_db,
        security_service: std::sync::Arc::new(xavier::app::security_service::SecurityService::new()),
        code_graph_dump_path: None,
    };

    let workspace_state = xavier::workspace::WorkspaceState::new(
        xavier::workspace::WorkspaceConfig {
            id: format!("rbac-mcp-{}", ulid::Ulid::new()),
            token: "root_token_123".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::File,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            dedup: xavier::settings::types::DedupSettings::default(),
        },
        xavier::agents::RuntimeConfig::default(),
        std::env::temp_dir().join(format!("rbac_mcp_store_{}", ulid::Ulid::new())),
    )
    .await
    .unwrap();

    let workspace_ctx = xavier::workspace::WorkspaceContext {
        workspace_id: workspace_state.config().id.clone(),
        workspace: std::sync::Arc::new(workspace_state),
    };

    // 1. Readonly call on 'create_memory' -> Err (Forbidden)
    let result = xavier::server::mcp::server::handle_tool_call(
        app_state.clone(),
        workspace_ctx.clone(),
        Some(&claims),
        "create_memory",
        json!({"path": "test/path", "content": "test content"}),
    )
    .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Forbidden") || err_msg.contains("Insufficient permissions"));

    // 2. Admin call on 'create_memory' -> Ok
    let admin_user = User::new(
        "admin@swal.dev".to_string(),
        "Admin User".to_string(),
        UserRole::Admin,
    );
    let admin_token = generate_jwt(&admin_user, jwt_secret.as_bytes()).unwrap();
    let admin_claims = validate_jwt(&admin_token, jwt_secret.as_bytes()).unwrap();

    let result = xavier::server::mcp::server::handle_tool_call(
        app_state,
        workspace_ctx,
        Some(&admin_claims),
        "create_memory",
        json!({"path": "test/path", "content": "test content"}),
    )
    .await;

    assert!(result.is_ok());
}
