//! Xavier administration web panel
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.

pub mod assets;
pub mod chat;
pub mod storage;
pub mod threads;
pub mod types;

pub use assets::{panel_asset, panel_index};
pub use chat::process_chat;
pub use storage::{
    get_graph, list_bookmarks, list_widgets, save_bookmark, save_graph, save_widget,
};
pub use threads::{create_thread, delete_thread, get_thread, list_threads};
pub use types::{
    Bookmark, CreateThreadRequest, GraphData, PanelChatRequest, PanelChatResponse, Widget,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codebase::connection_manager::ConnectionManager;
    use crate::codebase::conversations_db::{ThreadDetail, ThreadSummary};
    use crate::memory::sqlite_store::{
        TABLE_PANEL_BOOKMARKS, TABLE_PANEL_GRAPHS, TABLE_PANEL_WIDGETS,
    };
    use crate::{
        agents::RuntimeConfig,
        memory::file_indexer::{FileIndexer, FileIndexerConfig},
        workspace::{
            EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext,
            WorkspaceRegistry, WorkspaceState,
        },
        AppState,
    };
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        routing::{get, post},
        Extension, Router,
    };
    use chrono::Utc;
    use std::sync::Arc;
    use tower::util::ServiceExt;
    use ulid::Ulid;

    async fn test_state() -> (AppState, WorkspaceContext) {
        let workspace_id = format!("panel-test-{}", Ulid::new());
        let db_path = std::env::temp_dir().join(format!("xavier-panel-{}.db", Ulid::new()));
        let code_db = Arc::new(code_graph::db::CodeGraphDB::new(&db_path).expect("test assertion"));
        let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
        let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
        let workspace_registry = Arc::new(WorkspaceRegistry::new());
        let workspace = WorkspaceState::new(
            WorkspaceConfig {
                id: workspace_id.clone(),
                token: "panel-token".to_string(),
                plan: PlanTier::Personal,
                memory_backend: crate::memory::store::MemoryBackend::File,
                storage_limit_bytes: Some(10 * 1024 * 1024),
                request_limit: Some(10_000),
                request_unit_limit: Some(20_000),
                embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
                managed_google_embeddings: false,
                sync_policy: SyncPolicy::CloudMirror,
                dedup: crate::settings::types::DedupSettings::default(),
            },
            RuntimeConfig::default(),
            std::env::temp_dir().join(format!("xavier-panel-store-{}", Ulid::new())),
        )
        .await
        .expect("test assertion");
        workspace_registry
            .insert(workspace)
            .await
            .expect("test assertion");
        let workspace = workspace_registry
            .authenticate("panel-token")
            .await
            .expect("test assertion");

        // Initialize connection pool for tests
        let project_id = storage::resolve_panel_project_id(&workspace);
        let temp_dir = tempfile::tempdir().expect("test assertion");
        let db_root = temp_dir.path().to_path_buf();

        ConnectionManager::global()
            .connect(&project_id, db_root.to_str().unwrap())
            .expect("test assertion");

        // Initialize schema for panel tables
        ConnectionManager::global()
            .with_conn(&project_id, move |conn| {
                conn.execute_batch(&format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                        id TEXT PRIMARY KEY,
                        workspace_id TEXT NOT NULL,
                        title TEXT NOT NULL,
                        url TEXT NOT NULL,
                        metadata TEXT NOT NULL DEFAULT '{{}}',
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS {} (
                        id TEXT PRIMARY KEY,
                        workspace_id TEXT NOT NULL,
                        type TEXT NOT NULL,
                        config TEXT NOT NULL DEFAULT '{{}}',
                        x INTEGER DEFAULT 0,
                        y INTEGER DEFAULT 0,
                        w INTEGER DEFAULT 1,
                        h INTEGER DEFAULT 1,
                        created_at TEXT NOT NULL
                    );
                    CREATE TABLE IF NOT EXISTS {} (
                        id TEXT PRIMARY KEY,
                        workspace_id TEXT NOT NULL,
                        name TEXT NOT NULL,
                        data TEXT NOT NULL DEFAULT '{{}}',
                        created_at TEXT NOT NULL
                    );",
                    TABLE_PANEL_BOOKMARKS, TABLE_PANEL_WIDGETS, TABLE_PANEL_GRAPHS
                ))?;
                Ok(())
            })
            .await
            .expect("test assertion");

        // Keep the temp dir alive for the duration of the test
        Box::leak(Box::new(temp_dir));

        (
            AppState {
                workspace_registry,
                indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
                agent_indexer: crate::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
                    FileIndexerConfig::default(),
                    Some(code_indexer.clone()),
                )),
                code_indexer,
                code_query,
                code_db,
                security_service: Arc::new(crate::app::security_service::SecurityService::new()),
                code_graph_dump_path: None,
            },
            workspace,
        )
    }

    fn test_router(state: AppState, workspace: WorkspaceContext) -> Router {
        Router::new()
            .route("/panel/api/threads", get(list_threads).post(create_thread))
            .route(
                "/panel/api/threads/{thread_id}",
                get(get_thread).delete(delete_thread),
            )
            .route("/panel/api/chat", post(process_chat))
            .route(
                "/panel/api/bookmarks",
                get(list_bookmarks).post(save_bookmark),
            )
            .route("/panel/api/widgets", get(list_widgets).post(save_widget))
            .route("/panel/api/graph", get(get_graph).post(save_graph))
            .layer(Extension(workspace))
            .with_state(state)
    }

    #[tokio::test]
    async fn creates_and_fetches_threads_via_http() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        // Bind TcpListener to an ephemeral (random) port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind ephemeral port");
        let addr = listener.local_addr().expect("failed to get local address");
        let port = addr.port();

        // Spawn axum server on background task
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("failed to run axum serve");
        });

        // Build a client to make actual HTTP requests
        let client = reqwest::Client::new();

        // POST request to create a thread
        let create_url = format!("http://127.0.0.1:{}/panel/api/threads", port);
        let create_response = client
            .post(&create_url)
            .json(&serde_json::json!({ "title": "Panel Thread" }))
            .send()
            .await
            .expect("failed to send POST request");

        assert_eq!(create_response.status(), reqwest::StatusCode::OK);
        let summary: ThreadSummary = create_response
            .json()
            .await
            .expect("failed to parse thread summary");

        // GET request to fetch the created thread
        let get_url = format!("http://127.0.0.1:{}/panel/api/threads/{}", port, summary.id);
        let get_response = client
            .get(&get_url)
            .send()
            .await
            .expect("failed to send GET request");

        assert_eq!(get_response.status(), reqwest::StatusCode::OK);
        let detail: ThreadDetail = get_response
            .json()
            .await
            .expect("failed to parse thread detail");

        assert_eq!(detail.thread.id, summary.id);

        // Clean up background server task
        server_handle.abort();
    }

    #[tokio::test]
    async fn chat_persists_assistant_ui_message() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);
        let request = Request::builder()
            .method("POST")
            .uri("/panel/api/chat")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"message":"Explain xavier memory"}"#))
            .expect("test assertion");

        let response = app.oneshot(request).await.expect("test assertion");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let body_str = String::from_utf8_lossy(&body);
        println!("Response body: {}", body_str);
        let payload: PanelChatResponse = serde_json::from_slice(&body).expect("test assertion");

        assert_eq!(payload.messages.len(), 2);
        assert_eq!(payload.messages[1].role, "assistant");
        assert!(payload.messages[1].openui_lang.is_some());
    }

    #[tokio::test]
    async fn bookmarks_crud() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        let bookmark = Bookmark {
            id: "test-id".to_string(),
            title: "Test Bookmark".to_string(),
            url: "https://example.com".to_string(),
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        };

        let create_request = Request::builder()
            .method("POST")
            .uri("/panel/api/bookmarks")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&bookmark).unwrap()))
            .expect("test assertion");

        let response = app
            .clone()
            .oneshot(create_request)
            .await
            .expect("test assertion");
        assert_eq!(response.status(), StatusCode::OK);

        let get_request = Request::builder()
            .method("GET")
            .uri("/panel/api/bookmarks")
            .body(Body::empty())
            .expect("test assertion");

        let response = app.oneshot(get_request).await.expect("test assertion");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let bookmarks: Vec<Bookmark> = serde_json::from_slice(&body).expect("test assertion");
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].id, "test-id");
    }

    #[tokio::test]
    async fn widgets_crud() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        let widget = Widget {
            id: "widget-1".to_string(),
            widget_type: "chart".to_string(),
            config: serde_json::json!({"metric": "cpu"}),
            x: 0,
            y: 0,
            w: 4,
            h: 3,
            created_at: Utc::now(),
        };

        let create_request = Request::builder()
            .method("POST")
            .uri("/panel/api/widgets")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&widget).unwrap()))
            .expect("test assertion");

        let response = app
            .clone()
            .oneshot(create_request)
            .await
            .expect("test assertion");
        assert_eq!(response.status(), StatusCode::OK);

        let get_request = Request::builder()
            .method("GET")
            .uri("/panel/api/widgets")
            .body(Body::empty())
            .expect("test assertion");

        let response = app.oneshot(get_request).await.expect("test assertion");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let widgets: Vec<Widget> = serde_json::from_slice(&body).expect("test assertion");
        assert_eq!(widgets.len(), 1);
        assert_eq!(widgets[0].id, "widget-1");
        assert_eq!(widgets[0].widget_type, "chart");
    }

    #[tokio::test]
    async fn graphs_crud() {
        let (state, workspace) = test_state().await;
        let app = test_router(state, workspace);

        // Empty workspace returns 200 + empty roadmap (not 404).
        let empty_get = Request::builder()
            .method("GET")
            .uri("/panel/api/graph")
            .body(Body::empty())
            .expect("test assertion");
        let empty_response = app
            .clone()
            .oneshot(empty_get)
            .await
            .expect("test assertion");
        assert_eq!(empty_response.status(), StatusCode::OK);
        let empty_body = to_bytes(empty_response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let empty_graph: GraphData = serde_json::from_slice(&empty_body).expect("test assertion");
        assert_eq!(empty_graph.id, "default");
        assert_eq!(
            empty_graph.data["nodes"].as_array().map(|a| a.len()),
            Some(0)
        );
        assert_eq!(
            empty_graph.data["links"].as_array().map(|a| a.len()),
            Some(0)
        );

        let graph = GraphData {
            id: "graph-1".to_string(),
            name: "Workspace roadmap".to_string(),
            data: serde_json::json!({"nodes": [], "links": []}),
            created_at: Utc::now(),
        };

        let create_request = Request::builder()
            .method("POST")
            .uri("/panel/api/graph")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&graph).unwrap()))
            .expect("test assertion");

        let response = app
            .clone()
            .oneshot(create_request)
            .await
            .expect("test assertion");
        assert_eq!(response.status(), StatusCode::OK);

        // Reject legacy `edges` shape without `links`.
        let bad = GraphData {
            id: "graph-bad".to_string(),
            name: "bad".to_string(),
            data: serde_json::json!({"nodes": [], "edges": []}),
            created_at: Utc::now(),
        };
        let bad_request = Request::builder()
            .method("POST")
            .uri("/panel/api/graph")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&bad).unwrap()))
            .expect("test assertion");
        let bad_response = app
            .clone()
            .oneshot(bad_request)
            .await
            .expect("test assertion");
        assert_eq!(bad_response.status(), StatusCode::BAD_REQUEST);

        let get_request = Request::builder()
            .method("GET")
            .uri("/panel/api/graph")
            .body(Body::empty())
            .expect("test assertion");

        let response = app.oneshot(get_request).await.expect("test assertion");
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("test assertion");
        let result: GraphData = serde_json::from_slice(&body).expect("test assertion");
        assert_eq!(result.id, "graph-1");
        assert_eq!(result.name, "Workspace roadmap");
    }
}
