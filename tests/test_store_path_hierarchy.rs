use axum::{body::Body, http::Request, routing::post, Router};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
use xavier::cli::handlers::memory::{add_handler, search_handler};
use xavier::cli::state::{CliState, CodeGraphState};
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::coordination::KeyLendingEngine;
use xavier::coordination::SimpleAgentRegistry;
use xavier::embedding::NoopEmbedder;
use xavier::memory::agent_indexer::AgentIndexer;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::ports::inbound::AgentLifecyclePort;
use xavier::secrets::audit::QmdAuditLogger;
use xavier::tasks::store::{InMemoryTaskStore, TaskService};

async fn create_test_cli_state(temp_dir: &TempDir) -> CliState {
    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
    let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

    let db_path = temp_dir.path().join("test_store.db");
    let store_config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 1536,
    };
    let store = Arc::new(VecSqliteMemoryStore::new(store_config).await.unwrap());

    let cg_db = Arc::new(code_graph::db::CodeGraphDB::in_memory().unwrap());
    let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
        db: cg_db.clone(),
        indexer: Arc::new(code_graph::indexer::Indexer::new(cg_db.clone())),
        query: Arc::new(code_graph::query::QueryEngine::new(cg_db)),
    }));

    CliState {
        memory: memory_port,
        qmd_memory,
        store,
        workspace_id: "test-ws".to_string(),
        workspace_dir: temp_dir.path().to_path_buf(),
        state_dir: temp_dir.path().to_path_buf(),
        auth_db: None,
        code_graph: cg_state,
        security: Arc::new(xavier::app::security_service::SecurityService::new()),
        security_scan: Arc::new(xavier::app::security_service::SecurityService::new()),
        _time_store: None,
        agent_registry: SimpleAgentRegistry::new(None) as Arc<dyn AgentLifecyclePort>,
        panel_store: Arc::new(
            ConversationsDb::open_in_memory("test-project")
                .await
                .unwrap(),
        ),
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
        provider_router: Arc::new(tokio::sync::RwLock::new(ProviderRouter::new(
            ProviderKind::Local,
        ))),
        embedder: Arc::new(NoopEmbedder),
        agent_indexer: Arc::new(AgentIndexer::new(FileIndexer::new(
            FileIndexerConfig::default(),
            None,
        ))),
        auth_store: None,
        openclaw_indexer: Arc::new(xavier::memory::openclaw_indexer::OpenClawAgentIndexer::new(
            Arc::new(NoopEmbedder),
        )),
        multi_db: xavier::storage::multi_db::MultiDbManager::new(),
        system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
        maloca: xavier::maloca::MalocaStore::open(&temp_dir.path().join("xavier-maloca")),
    }
}

/// Requirement 1: Path stored verbatim (slashes and accents preserved, backslashes normalized)
#[tokio::test]
async fn test_path_stored_verbatim() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .with_state(state.clone());

    let canonical_path = "hermes/2026-08-17/categoría/diseño.md";
    let payload = serde_json::json!({
        "content": "Contenido de prueba con acentos y jerarquía",
        "path": canonical_path,
        "title": "Nota de diseño"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/memory/add")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(res_json["status"], "ok");
    assert_eq!(res_json["path"], canonical_path);

    // Verify stored in QmdMemory domain & store verbatim
    let doc = state.qmd_memory.get(canonical_path).await.unwrap();
    assert!(
        doc.is_some(),
        "Document with verbatim canonical path should be persisted in QmdMemory"
    );
    assert_eq!(doc.unwrap().path, canonical_path);
}

/// Requirement 2: Path traversal blocked (../../etc/passwd replaced with fallback memory/{ulid})
#[tokio::test]
async fn test_path_traversal_blocked() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .with_state(state.clone());

    let traversal_paths = vec![
        "../../etc/passwd",
        "hermes/../../secret.txt",
        "../dir/../file.md",
    ];

    for path in traversal_paths {
        let payload = serde_json::json!({
            "content": "Path traversal attempt",
            "path": path
        });

        let req = Request::builder()
            .method("POST")
            .uri("/memory/add")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(res_json["status"], "ok");
        let assigned_path = res_json["path"].as_str().unwrap();

        assert!(
            assigned_path.starts_with("memory/"),
            "Traversal path should fall back to memory/ULID, got: {}",
            assigned_path
        );
        assert!(
            !assigned_path.contains(".."),
            "Assigned path must not contain traversal parent markers '..'"
        );
    }
}

/// Requirement 3: NUL byte injection blocked
#[tokio::test]
async fn test_nul_byte_injection_blocked() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .with_state(state.clone());

    let payload = serde_json::json!({
        "content": "NUL byte test content",
        "path": "hermes/subfolder\0/exploit.txt"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/memory/add")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(res_json["status"], "ok");
    let assigned_path = res_json["path"].as_str().unwrap();

    assert!(
        assigned_path.starts_with("memory/"),
        "NUL byte injection path should fall back to safe memory/ULID path, got: {}",
        assigned_path
    );
    assert!(
        !assigned_path.contains('\0'),
        "Path must not contain NUL bytes"
    );
}

/// Requirement 4: Valid nested directory paths
#[tokio::test]
async fn test_valid_nested_directory_paths() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .with_state(state.clone());

    let valid_paths = vec![
        "hermes/2026-08-17/logs/daily.log",
        "swal/core/config/settings.json",
        "deeply/nested/directory/structure/file.txt",
    ];

    for path in valid_paths {
        let payload = serde_json::json!({
            "content": format!("Content for {}", path),
            "path": path
        });

        let req = Request::builder()
            .method("POST")
            .uri("/memory/add")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let res_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(res_json["status"], "ok");
        assert_eq!(res_json["path"], path);
    }
}

/// Requirement 5: Path hierarchy queries (parent/child)
#[tokio::test]
async fn test_path_hierarchy_queries() {
    let temp = tempfile::tempdir().unwrap();
    let state = create_test_cli_state(&temp).await;
    let app = Router::new()
        .route("/memory/add", post(add_handler))
        .route("/memory/search", post(search_handler))
        .with_state(state.clone());

    // Add multiple files in hierarchy
    let files = vec![
        ("projects/alpha/readme.md", "Project Alpha documentation"),
        ("projects/alpha/src/main.rs", "Project Alpha source code"),
        ("projects/beta/readme.md", "Project Beta documentation"),
    ];

    for (path, content) in files {
        let payload = serde_json::json!({
            "content": content,
            "path": path
        });

        let req = Request::builder()
            .method("POST")
            .uri("/memory/add")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    // 1. Verify path_prefix filtering via schema query filters on QmdMemory search
    let filters = xavier::memory::schema::MemoryQueryFilters {
        path_prefix: Some("projects/alpha".to_string()),
        ..Default::default()
    };

    let results = state
        .qmd_memory
        .search_filtered("Project", 100, Some(&filters))
        .await
        .unwrap();

    assert_eq!(
        results.len(),
        2,
        "Should return 2 files under projects/alpha"
    );
    let paths: Vec<String> = results.into_iter().map(|r| r.path).collect();
    assert!(paths.contains(&"projects/alpha/readme.md".to_string()));
    assert!(paths.contains(&"projects/alpha/src/main.rs".to_string()));
    assert!(!paths.contains(&"projects/beta/readme.md".to_string()));

    // 2. Verify QmdMemory `ls` parent/child hierarchy navigation
    let nav_entries = state.qmd_memory.ls("projects/alpha/").await.unwrap();
    assert!(
        !nav_entries.is_empty(),
        "ls on parent directory should return entries"
    );
}
