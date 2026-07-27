//! MCP transports — stdio and HTTP+SSE entry points.
//!
//! Both transports share a single [`build_mcp_state`] that initializes the
//! memory store, builds the shared application state, and authenticates the
//! MCP workspace. The actual JSON-RPC dispatch lives in
//! [`xavier::server::mcp::session::dispatch_mcp_value`], so both transports
//! expose identical protocol behavior with no duplicated logic.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::app::security_service::SecurityService;
use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};
use xavier::memory::sqlite_vec_store::VecSqliteMemoryStore;
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::server::mcp::transport::start_mcp_http_server;
use xavier::server::mcp_stdio::run_stdio_loop;
use xavier::workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState};
use xavier::AppState;

use crate::cli::config::resolve_http_bind_host;

/// Default port for the MCP HTTP+SSE server.
pub const DEFAULT_MCP_PORT: u16 = 8100;

/// Build the shared MCP application state and authenticated workspace.
///
/// Used by both the stdio and HTTP+SSE transports so they share identical
/// initialization and core wiring (memory store, security service, workspace).
pub async fn build_mcp_state() -> Result<(AppState, xavier::workspace::WorkspaceContext)> {
    // Initialize memory store (same as HTTP server)
    let store: Arc<dyn MemoryStore> = Arc::new(VecSqliteMemoryStore::from_env().await?);
    let workspace_id =
        std::env::var("XAVIER_DEFAULT_WORKSPACE_ID").unwrap_or_else(|_| "default".to_string());
    let durable_state = store.load_workspace_state(&workspace_id).await?;
    let docs = Arc::new(RwLock::new(
        durable_state
            .memories
            .iter()
            .map(MemoryRecord::to_document)
            .collect::<Vec<MemoryDocument>>(),
    ));
    let memory = Arc::new(QmdMemory::new_with_workspace(docs, workspace_id.clone()));
    memory.set_store(Arc::clone(&store)).await;
    memory.init().await?;

    // Build the shared application state so the unified MCP dispatcher has
    // everything it needs (security service, workspace registry, etc.).
    let security_service = Arc::new(SecurityService::new());
    let workspace_registry = Arc::new(WorkspaceRegistry::new());

    let code_db = Arc::new(code_graph::db::CodeGraphDB::in_memory()?);
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));

    let state = AppState {
        workspace_registry: Arc::clone(&workspace_registry),
        code_indexer: Arc::clone(&code_indexer),
        code_query,
        code_db,
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
        security_service,
        code_graph_dump_path: Some({
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            xavier::codebase::codegraph_paths::codegraph_dump_path_for(&cwd)
        }),
    };

    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: workspace_id.clone(),
            token: "mcp-token".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::Sqlite,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            dedup: xavier::settings::types::DedupSettings::default(),
        },
        xavier::agents::RuntimeConfig::default(),
        std::env::temp_dir().join("xavier-mcp"),
    )
    .await?;
    workspace_registry.insert(workspace).await?;
    let workspace = workspace_registry
        .authenticate("mcp-token")
        .await
        .ok_or_else(|| anyhow::anyhow!("failed to authenticate MCP workspace"))?;

    Ok((state, workspace))
}

/// `xavier mcp` — start the MCP stdio transport (for local/OpenClaw integration).
pub async fn start_mcp_stdio() -> Result<()> {
    let (state, workspace) = build_mcp_state().await?;
    run_stdio_loop(state, workspace).await
}

/// Start the MCP HTTP+SSE (Streamable HTTP) transport on `port`.
///
/// Binds to the resolved HTTP host (default `127.0.0.1`). Intended to be
/// spawned alongside the main HTTP server or run standalone.
pub async fn start_mcp_http(port: u16) -> Result<()> {
    let (state, workspace) = build_mcp_state().await?;
    let bind_host = resolve_http_bind_host();
    let bind_addr = format!("{bind_host}:{port}");
    tracing::info!("Starting Xavier MCP HTTP+SSE server on {}", bind_addr);
    start_mcp_http_server(state, workspace, bind_addr).await
}
