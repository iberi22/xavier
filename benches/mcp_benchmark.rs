use criterion::{criterion_group, criterion_main, Criterion};
use serde_json::json;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use xavier::workspace::WorkspaceContext;
use xavier::{
    agents::RuntimeConfig,
    memory::file_indexer::{FileIndexer, FileIndexerConfig},
    workspace::{WorkspaceConfig, WorkspaceState},
    AppState,
};

fn bench_mcp_ops(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");

    // Setup state
    let (state, workspace) = rt.block_on(async {
        let temp_dir = std::env::temp_dir().join(format!("xavier-mcp-bench-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).ok();

        let db_path = temp_dir.join("code_graph.db");
        let code_db = Arc::new(
            code_graph::db::CodeGraphDB::new(&db_path).expect("CodeGraphDB creation failed for bench"),
        );
        let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
        let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));

        let workspace = WorkspaceState::new(
            WorkspaceConfig {
                id: "bench-mcp-ws".to_string(),
                token: "bench-token".to_string(),
                plan: xavier::workspace::PlanTier::Pro,
                memory_backend: xavier::memory::MemoryBackend::File,
                storage_limit_bytes: None,
                request_limit: None,
                request_unit_limit: None,
                embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
                managed_google_embeddings: false,
                sync_policy: xavier::workspace::SyncPolicy::LocalOnly,
            },
            RuntimeConfig::default(),
            temp_dir.join("threads"),
        )
        .await
        .expect("WorkspaceState creation failed for bench");

        let context = WorkspaceContext {
            workspace_id: "bench-mcp-ws".to_string(),
            workspace: Arc::new(workspace),
        };

        // Seed some mock memories
        for i in 0..10 {
            let _ = context.workspace.memory.add_document(
                format!("bench/mcp/doc/{}", i),
                format!("This is MCP benchmark document number {} containing some test content.", i),
                serde_json::json!({"index": i})
            ).await;
        }

        let state = AppState {
            workspace_registry: Arc::new(xavier::workspace::WorkspaceRegistry::new()),
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

        (state, context)
    });

    // 1. Benchmark list_tools payload generation
    c.bench_function("mcp_list_tools_payload_generation", |b| {
        b.iter(|| {
            let tools = xavier::server::mcp::tools_core::list_tools_metadata();
            assert!(tools.len() >= 16);
        });
    });

    // 2. Benchmark MCP memory search routing & query execution
    c.bench_function("mcp_memory_search_execution", |b| {
        b.iter(|| {
            rt.block_on(async {
                let results = workspace.workspace.memory.search("MCP benchmark document", 5).await.unwrap_or_default();
                assert!(!results.is_empty());
            });
        });
    });
}

criterion_group!(mcp_benches, bench_mcp_ops);
criterion_main!(mcp_benches);
