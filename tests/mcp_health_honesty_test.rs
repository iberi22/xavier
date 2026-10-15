xavier::isolate_test_process!();

use axum::{body::Body, http::Request, routing::post, Router};
use serde_json::{json, Value};
use std::{ffi::OsString, path::PathBuf, sync::Arc};
use tower::util::ServiceExt;
use xavier::{
    agents::RuntimeConfig,
    memory::file_indexer::{FileIndexer, FileIndexerConfig},
    workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState},
    AppState,
};

struct Environment {
    original: Vec<(OsString, OsString)>,
    cwd: PathBuf,
}

impl Environment {
    fn isolate(root: &std::path::Path) -> Self {
        let guard = Self {
            original: std::env::vars_os().collect(),
            cwd: std::env::current_dir().unwrap(),
        };
        for (key, _) in &guard.original {
            if key.to_string_lossy().starts_with("XAVIER_") {
                std::env::remove_var(key);
            }
        }
        for key in [
            "HOME",
            "XDG_DATA_HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
            "XAVIER_DATA_DIR",
            "XAVIER_HOME",
            "XAVIER_CONFIG_DIR",
            "XAVIER_STATE_DIR",
        ] {
            std::env::set_var(key, root);
        }
        std::env::set_var("XAVIER_CONFIG_PATH", root.join("config.json"));
        std::env::set_var("XAVIER_TOKEN", "mcp-honesty-token");
        // Disabled is a deterministic unavailable provider, with no external requests/downloads.
        std::env::set_var("XAVIER_EMBEDDER", "disabled");
        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
        std::env::set_var("XAVIER_EMBEDDING_URL", "http://127.0.0.1:9/v1/embeddings");
        std::env::set_current_dir(root).unwrap();
        guard
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.cwd).unwrap();
        for (key, _) in std::env::vars_os() {
            if !self.original.iter().any(|(original, _)| original == &key) {
                std::env::remove_var(key);
            }
        }
        for (key, value) in &self.original {
            std::env::set_var(key, value);
        }
    }
}

async fn router(root: &std::path::Path) -> Router {
    let code_db = Arc::new(code_graph::db::CodeGraphDB::new(&root.join("code_graph.db")).unwrap());
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(code_db.clone()));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(code_db.clone()));
    let workspace_registry = Arc::new(WorkspaceRegistry::new());
    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: "test-mcp-honesty".into(),
            token: "mcp-honesty-token".into(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::File,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            protocol: Default::default(),
            dedup: Default::default(),
        },
        RuntimeConfig::default(),
        root.join("threads"),
    )
    .await
    .unwrap();
    workspace_registry.insert(workspace).await.unwrap();
    let workspace = workspace_registry
        .authenticate("mcp-honesty-token")
        .await
        .unwrap();
    let state = AppState {
        workspace_registry,
        indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
        agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
            FileIndexerConfig::default(),
            Some(code_indexer.clone()),
        )),
        code_indexer,
        code_query,
        code_db,
        security_service: Arc::new(xavier::app::security_service::SecurityService::new()),
        code_graph_dump_path: None,
    };
    Router::new()
        .route("/mcp", post(xavier::server::mcp::session::mcp_post_handler))
        .layer(axum::middleware::from_fn(
            xavier::server::mcp::auth::mcp_auth_middleware,
        ))
        .layer(axum::Extension(workspace))
        .with_state(state)
}

async fn call(router: Router, tool: &str) -> Value {
    let request = Request::post("/mcp")
        .header("content-type", "application/json")
        .header("Origin", "http://127.0.0.1:8080")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", tool)
        .header("X-Xavier-Token", "mcp-honesty-token")
        .body(Body::from(
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": tool, "arguments": {} }
            })
            .to_string(),
        ))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body.get("error").is_none(), "MCP error: {body}");
    assert!(body["result"]["structuredContent"].is_object(), "{body}");
    body["result"]["structuredContent"].clone()
}

#[tokio::test]
async fn mcp_reports_unmeasured_integrity_and_unavailable_embedding_honestly() {
    let root = tempfile::Builder::new()
        .prefix("xavier-test-mcp-health-")
        .tempdir()
        .unwrap();
    let _environment = Environment::isolate(root.path());
    let app = router(root.path()).await;
    let health = call(app.clone(), "sys_health").await;
    assert_eq!(health.get("db_integrity"), Some(&Value::Null));
    let gaps = health["active_gaps"].as_array().expect("active_gaps array");
    for gap in gaps {
        let metric = gap["metric"].as_str().expect("gap metric");
        assert!(
            ![
                "recall@k",
                "precision",
                "avg_latency",
                "cache_hit_rate",
                "db_integrity"
            ]
            .contains(&metric),
            "unmeasured metric became a measured gap: {gap}"
        );
    }
    assert_eq!(health["components"]["embedding"]["connected"], false);
    assert_eq!(health["components"]["embedding"]["fallback_success"], false);
    let check = call(app, "health_check").await;
    assert_eq!(check["embeddingOk"], false);
    assert_eq!(check["memoryStoreOk"], true);
    let status = check["status"].as_str().expect("health status");
    assert!(["degraded", "unhealthy", "critical"].contains(&status));
}
