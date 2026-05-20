//! MCP stdio server — delegates to the unified dispatch in server::mcp_server
//!
//! This is the entry point for `xavier mcp`.  It initializes the memory store,
//! builds the shared application state, and hands control to the same
//! `dispatch_mcp_value` that the HTTP /mcp endpoint uses.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use xavier::app::security_service::SecurityService;
use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};
use xavier::memory::sqlite_vec_store::VecSqliteMemoryStore;
use xavier::memory::store::{MemoryRecord, MemoryStore};
use xavier::server::mcp_stdio::run_stdio_loop;
use xavier::workspace::{WorkspaceConfig, WorkspaceRegistry, WorkspaceState};
use xavier::AppState;

pub async fn start_mcp_stdio() -> Result<()> {
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

    let state = AppState {
        workspace_registry: Arc::clone(&workspace_registry),
        code_indexer: Arc::new(code_graph::indexer::Indexer::new(Arc::new(
            code_graph::db::CodeGraphDB::in_memory()?,
        ))),
        code_query: Arc::new(code_graph::query::QueryEngine::new(Arc::new(
            code_graph::db::CodeGraphDB::in_memory()?,
        ))),
        code_db: Arc::new(code_graph::db::CodeGraphDB::in_memory()?),
        indexer: xavier::memory::file_indexer::FileIndexer::new(
            xavier::memory::file_indexer::FileIndexerConfig::default(),
            None,
        ),
        pattern_adapter: Arc::new(
            xavier::adapters::outbound::vec::pattern_adapter::PatternAdapter::new(),
        ),
        security_service,
    };

    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: workspace_id.clone(),
            token: "mcp-stdio-token".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::Sqlite,
            storage_limit_bytes: None,
            request_limit: None,
            request_unit_limit: None,
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
        },
        xavier::agents::RuntimeConfig::default(),
        std::env::temp_dir().join("xavier-mcp-stdio"),
    )
    .await?;
    workspace_registry.insert(workspace).await?;
    let workspace = workspace_registry
        .authenticate("mcp-stdio-token")
        .await
        .ok_or_else(|| anyhow::anyhow!("failed to authenticate MCP stdio workspace"))?;

    // Delegate to the unified MCP stdio loop.
    run_stdio_loop(state, workspace).await
}
