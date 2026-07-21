//! Navigation API handlers for shell-like interaction (ls, cd, pwd)

use crate::cli::state::CliState;
use crate::memory::graph_traversal::Pathfinder;
use crate::workspace::WorkspaceContext;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
pub struct LsParams {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CdParams {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct AffectedParams {
    pub path: String,
    pub depth: Option<usize>,
    pub exclude_file_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TelemetryParams {
    /// When true, return only the top-N hotspots instead of the full summary.
    pub hotspots: Option<bool>,
    /// Number of hotspot entries to include (default 10).
    pub top: Option<usize>,
}

/// Ls handler.
pub async fn ls_handler(
    State(state): State<CliState>,
    Query(params): Query<LsParams>,
) -> impl IntoResponse {
    let path = params.path.unwrap_or_default();
    match state.memory.ls(&path).await {
        Ok(entries) => Json(json!({
            "status": "ok",
            "path": path,
            "entries": entries
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": e.to_string()
            })),
        )
            .into_response(),
    }
}

/// Cd handler.
pub async fn cd_handler(
    State(state): State<CliState>,
    Json(params): Json<CdParams>,
) -> impl IntoResponse {
    // CD in the API context just validates if the path "exists" as a directory or document
    match state.memory.ls(&params.path).await {
        Ok(entries) => {
            if entries.is_empty() {
                // If it's empty, we should check if the path itself is a document
                match state.memory.get(&params.path).await {
                    Ok(Some(_)) => Json(json!({
                        "status": "ok",
                        "path": params.path,
                        "is_doc": true,
                        "is_dir": false
                    }))
                    .into_response(),
                    _ => (
                        StatusCode::NOT_FOUND,
                        Json(json!({
                            "status": "error",
                            "message": "Path not found"
                        })),
                    )
                        .into_response(),
                }
            } else {
                Json(json!({
                    "status": "ok",
                    "path": params.path,
                    "is_doc": false,
                    "is_dir": true
                }))
                .into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "status": "error",
                "message": e.to_string()
            })),
        )
            .into_response(),
    }
}

/// Pwd handler.
pub async fn pwd_handler() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "message": "PWD is handled client-side in the CLI, but the API is ready for navigation."
    }))
}

/// Affected handler.
pub async fn affected_handler(
    Extension(ctx): Extension<WorkspaceContext>,
    Query(params): Query<AffectedParams>,
) -> impl IntoResponse {
    let depth = params.depth.unwrap_or(2);
    let graph_guard = ctx.workspace.belief_graph.read().await;
    let pathfinder = Pathfinder::new(&graph_guard);

    // 1. Resolve start nodes.
    // We try to find nodes that correspond to the given path/concept.
    // If it's a document path, we look for beliefs with that provenance.
    let mut start_nodes = HashSet::new();

    // Check if it's a known concept (node)
    if graph_guard.get_node(&params.path).is_some() {
        start_nodes.insert(params.path.clone());
    }

    // Also check for provenance matching
    for edge in graph_guard.get_edges() {
        if edge.provenance_id == params.path
            || edge.source == params.path
            || edge.target == params.path
        {
            start_nodes.insert(edge.source.clone());
            start_nodes.insert(edge.target.clone());
        }
    }

    if start_nodes.is_empty() {
        // Try searching for the document to see if we can find more context
        match ctx.workspace.memory.get(&params.path).await {
            Ok(Some(doc)) => {
                // If we found a document, use its path as a potential seed
                start_nodes.insert(doc.path.clone());
            }
            _ => {}
        }
    }

    let mut all_affected = Vec::new();
    let mut seen_nodes = HashSet::new();

    for start_node in start_nodes {
        let affected = pathfinder.affected_bfs(&start_node, depth);
        for item in affected {
            if seen_nodes.insert(item.node.clone()) {
                // Apply filters
                if let Some(ref exclude) = params.exclude_file_type {
                    if exclude == "code" {
                        // Basic heuristic: check if node name looks like a code symbol or file
                        if item.node.contains("::")
                            || item.node.ends_with(".rs")
                            || item.node.ends_with(".py")
                            || item.node.ends_with(".js")
                            || item.node.ends_with(".ts")
                        {
                            continue;
                        }
                    }
                }
                all_affected.push(item);
            }
        }
    }

    Json(json!({
        "status": "ok",
        "path": params.path,
        "affected": all_affected
    }))
}

/// Visualize handler.
pub async fn visualize_handler(Extension(ctx): Extension<WorkspaceContext>) -> impl IntoResponse {
    let graph_guard = ctx.workspace.belief_graph.read().await;
    let edges = graph_guard.get_edges();

    let all_docs = ctx.workspace.memory.all_documents().await;

    let policy = ctx.workspace.hormer.policy().read().await;
    let weights = &policy.layer_weights;
    let traversal_weights = &policy.traversal_weights;

    let telemetry = ctx.workspace.hormer.telemetry();
    let hotspots = telemetry.get_hotspots(10).await;

    // Compute a per-document HORMER score based on policy weights and telemetry.
    let hormer_scores: std::collections::HashMap<String, f64> = {
        let mut visited_nodes = std::collections::HashMap::new();
        for (node, info) in &hotspots {
            visited_nodes.insert(node.clone(), info.count as f64);
        }
        all_docs
            .iter()
            .map(|doc| {
                let visit_factor = visited_nodes.get(&doc.path).copied().unwrap_or(0.0);
                let layer_avg = (weights.working + weights.episodic + weights.semantic) / 3.0;
                let score = visit_factor * layer_avg as f64;
                (doc.path.clone(), score)
            })
            .collect()
    };

    let summary = telemetry.get_summary().await;

    Json(json!({
        "status": "ok",
        "workspace_id": ctx.workspace_id,
        "documents": all_docs,
        "edges": edges,
        "weights": {
            "working": weights.working,
            "episodic": weights.episodic,
            "semantic": weights.semantic,
        },
        "traversal_weights": traversal_weights,
        "hotspots": hotspots,
        "hormer_scores": hormer_scores,
        "metrics": {
            "total_visits": summary.total_visits,
            "unique_nodes": summary.unique_nodes,
            "avg_path_length": summary.avg_path_length,
            "total_paths": summary.total_paths,
            "nav_score_histogram": summary.nav_score_histogram,
        }
    }))
}

/// Telemetry handler.
pub async fn telemetry_handler(
    Extension(ctx): Extension<WorkspaceContext>,
    Query(params): Query<TelemetryParams>,
) -> impl IntoResponse {
    let telemetry = ctx.workspace.hormer.telemetry();
    let top = params.top.unwrap_or(10).max(1);

    if params.hotspots.unwrap_or(false) {
        let hotspots = telemetry.get_hotspots(top).await;
        let summary = telemetry.get_summary().await;
        Json(json!({
            "status": "ok",
            "hotspots": hotspots,
            "metrics": {
                "total_visits": summary.total_visits,
                "unique_nodes": summary.unique_nodes,
                "avg_path_length": summary.avg_path_length,
                "total_paths": summary.total_paths,
                "nav_score_histogram": summary.nav_score_histogram,
            }
        }))
        .into_response()
    } else {
        let summary = telemetry.get_summary().await;
        Json(json!({
            "status": "ok",
            "telemetry": summary,
        }))
        .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::state::{CliState, CodeGraphState};
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock as AsyncRwLock;
    use tower::util::ServiceExt;
    use xavier::agents::rate_limit::RateLimitManager;
    use xavier::app::proxy_use_case::ProxyUseCase;
    use xavier::app::qmd_memory_adapter::QmdMemoryAdapter;
    use xavier::codebase::conversations_db::ConversationsDb;
    use xavier::coordination::KeyLendingEngine;
    use xavier::coordination::SimpleAgentRegistry;
    use xavier::coordination::XavierEventBus;
    use xavier::embedding::NoopEmbedder;
    use xavier::memory::agent_indexer::AgentIndexer;
    use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
    use xavier::memory::qmd_memory::QmdMemory;
    use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
    use xavier::security::sessions::SessionManager;
    use xavier::tasks::store::{InMemoryTaskStore, TaskService};

    async fn test_state() -> CliState {
        use xavier::agents::provider::router::{ProviderKind, ProviderRouter};
        use xavier::app::security_service::SecurityService;
        use xavier::memory::sqlite_vec_store::DEFAULT_EMBEDDING_DIMENSIONS;
        use xavier::secrets::audit::QmdAuditLogger;

        let docs = Arc::new(AsyncRwLock::new(Vec::new()));
        let qmd_memory = Arc::new(QmdMemory::new_with_workspace(docs, "test-ws"));
        let memory_port = Arc::new(QmdMemoryAdapter::new(Arc::clone(&qmd_memory)));

        let config = VecSqliteStoreConfig {
            path: PathBuf::from(":memory:"),
            embedding_dimensions: DEFAULT_EMBEDDING_DIMENSIONS,
        };
        let store = Arc::new(VecSqliteMemoryStore::new(config).await.unwrap());

        let cg_db = Arc::new(code_graph::db::CodeGraphDB::new(&PathBuf::from(":memory:")).unwrap());
        let cg_state = Arc::new(tokio::sync::RwLock::new(CodeGraphState {
            db: cg_db.clone(),
            indexer: Arc::new(code_graph::indexer::Indexer::new(cg_db.clone())),
            query: Arc::new(code_graph::query::QueryEngine::new(cg_db)),
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
                    .unwrap(),
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
            usage_counters: Arc::new(xavier::observability::UsageCounters::new()),
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
            openclaw_indexer: Arc::new(
                xavier::memory::openclaw_indexer::OpenClawAgentIndexer::new(Arc::new(NoopEmbedder)),
            ),
            multi_db: xavier::storage::multi_db::MultiDbManager::new(),
            system_scan_cache: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    #[tokio::test]
    async fn test_nav_api() {
        let state = test_state().await;
        state
            .qmd_memory
            .add_document("docs/test".to_string(), "content".to_string(), json!({}))
            .await
            .unwrap();

        let app = Router::new()
            .route("/v1/nav/ls", get(ls_handler))
            .route("/v1/nav/cd", post(cd_handler))
            .with_state(state);

        // Test LS
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/nav/ls?path=docs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Test CD (valid)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/nav/cd")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"path": "docs"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Test CD (invalid)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/nav/cd")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"path": "nonexistent"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_visualize_api() {
        let state = test_state().await;
        let workspace_id = state.workspace_id.clone();

        use xavier::agents::RuntimeConfig;
        use xavier::memory::store::MemoryBackend;
        use xavier::workspace::{
            EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext,
            WorkspaceState,
        };

        let temp_dir = tempfile::tempdir().unwrap();
        let config = WorkspaceConfig {
            id: workspace_id.clone(),
            token: "test-token".to_string(),
            plan: PlanTier::Community,
            memory_backend: MemoryBackend::Memory,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::LocalOnly,
        };

        let workspace_state = Arc::new(
            WorkspaceState::new(config, RuntimeConfig::default(), temp_dir.path())
                .await
                .unwrap(),
        );

        let ctx = WorkspaceContext {
            workspace_id: workspace_id.clone(),
            workspace: workspace_state,
        };

        let app = Router::new()
            .route("/v1/nav/visualize", get(visualize_handler))
            .layer(axum::Extension(ctx))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/nav/visualize")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(resp.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["status"], "ok");
        assert!(body.get("documents").is_some());
        assert!(body.get("edges").is_some());
        assert!(body.get("weights").is_some());
    }
}
