//! Integration tests for Xavier Sprints 1-4
//!
//! Covers:
//! - Sprint 1: Unified Memory Search (BM25 fallback)
//! - Sprint 2: HTTP REST Search Endpoints
//! - Sprint 3: Auto-capture of Memory Events
//! - Sprint 4: Embedding Cache & TTL
//! - Backend: Persistence & Workspace Init

use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;
use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory, EmbeddingCacheEntry};
use xavier::memory::qmd_memory::config::EMBEDDING_CACHE;
use xavier::memory::store::{InMemoryMemoryStore, MemoryStore, MemoryRecord};
#[allow(unused_imports)]
use xavier::memory::schema::MemoryQueryFilters;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
    Extension,
};
use tower::util::ServiceExt;
use http_body_util::BodyExt;
use xavier::workspace::WorkspaceContext;
use xavier::AppState;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::agents::RuntimeConfig;
use xavier::workspace::{WorkspaceConfig, WorkspaceState};
use ulid::Ulid;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_test_path(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    std::env::temp_dir().join(format!("{}-{}", prefix, unique))
}

#[tokio::test]
async fn test_sprint1_unified_search_bm25_fallback() {
    // 1. Setup QmdMemory with an empty store
    let docs = Arc::new(AsyncRwLock::new(Vec::new()));
    let memory = QmdMemory::new_with_workspace(docs.clone(), "test-workspace");

    let store = Arc::new(InMemoryMemoryStore::new());
    memory.set_store(store as Arc<dyn MemoryStore>).await;

    // 2. Add a document directly to the in-memory 'docs' list, bypassing the store
    // This simulates data that might have been added via memory_save but not yet persisted
    // or data that for some reason is only in the in-memory cache.
    let doc = MemoryDocument {
        id: Some("doc-1".to_string()),
        path: "test/sprint1".to_string(),
        content: "Xavier unified search should find this via BM25 fallback even if store is empty.".to_string(),
        metadata: serde_json::json!({}),
        ..Default::default()
    };

    docs.write().await.push(doc);

    // 3. Search for the document
    // search_filtered should try hybrid/embedding/cache first, and finally fallback to bm25_search on docs
    let results = memory.search_filtered("unified search", 10, None).await.expect("search failed");

    // 4. Verify results
    assert!(!results.is_empty(), "Should have found at least one result via BM25 fallback");
    assert_eq!(results[0].path, "test/sprint1");
}

#[tokio::test]
async fn test_sprint2_http_rest_search() {
    // 1. Setup AppState and WorkspaceContext
    let unique_id = Ulid::new().to_string();
    let db_path = unique_test_path("xavier-test-sprint2-db");
    let code_db = Arc::new(code_graph::db::CodeGraphDB::new(&db_path).unwrap());
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));

    let workspace_state = Arc::new(
        WorkspaceState::new(
            WorkspaceConfig {
                id: format!("ws-{}", unique_id),
                token: "test-token".to_string(),
                plan: xavier::workspace::PlanTier::Personal,
                memory_backend: xavier::memory::store::MemoryBackend::Memory,
                storage_limit_bytes: None,
                request_limit: None,
                request_unit_limit: None,
                embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
                managed_google_embeddings: false,
                sync_policy: xavier::workspace::SyncPolicy::LocalOnly,
            },
            RuntimeConfig::default(),
            unique_test_path("xavier-test-sprint2-panel"),
        )
        .await
        .unwrap(),
    );

    let state = AppState {
        workspace_registry: Arc::new(xavier::workspace::WorkspaceRegistry::new()), // Dummy
        code_indexer,
        code_query,
        code_db,
        indexer: FileIndexer::new(FileIndexerConfig::default(), None),
        agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(
            FileIndexer::new(FileIndexerConfig::default(), None)
        ),
        security_service: Arc::new(xavier::app::security_service::SecurityService::new()),
        code_graph_dump_path: None,
        event_bus: xavier::coordination::events::XavierEventBus::new(10),
    };

    let workspace_ctx = WorkspaceContext {
        workspace_id: format!("ws-{}", unique_id),
        workspace: workspace_state.clone(),
    };

    // 2. Add some data
    workspace_state.memory.add_document(
        "api/test".to_string(),
        "REST API search should work".to_string(),
        serde_json::json!({"kind": "Context"}),
    ).await.unwrap();

    // 3. Setup Router with v1_api handlers
    let app = Router::new()
        .route("/v1/memories/search", post(xavier::server::v1_api::v1_memories_search))
        .layer(Extension(workspace_ctx))
        .with_state(state);

    // 4. Perform Request
    let request = Request::builder()
        .method("POST")
        .uri("/v1/memories/search")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({
            "query": "REST API",
            "limit": 5
        }).to_string()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // 5. Verify Response
    assert_eq!(json["status"], "ok");
    let results = json["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert!(results[0]["memory"].as_str().unwrap().contains("REST API"));
}

#[tokio::test]
async fn test_sprint3_auto_capture_events() {
    // Sprint 3 implemented auto-capture of events in writer.rs via emit_operation_event.
    // While this currently logs via tracing::info, we can verify the public API
    // triggers this logic.
    let docs = Arc::new(AsyncRwLock::new(Vec::new()));
    let memory = QmdMemory::new_with_workspace(docs.clone(), "test-events");

    // Add operation
    let doc = MemoryDocument {
        id: Some("event-doc".to_string()),
        path: "test/events".to_string(),
        content: "Auto-capture test".to_string(),
        metadata: serde_json::json!({"session_id": "session-123"}),
        ..Default::default()
    };

    // This calls writer::add which calls emit_operation_event
    memory.add(doc.clone()).await.expect("add failed");

    // Update operation
    let mut updated_doc = doc.clone();
    updated_doc.content = "Auto-capture test updated".to_string();
    memory.update(updated_doc).await.expect("update failed");

    // Delete operation
    memory.delete("test/events").await.expect("delete failed");

    // If these operations complete without error, the emit_operation_event
    // logic (which constructs SessionEvent) is at least smoke-tested for logic errors.
    assert_eq!(docs.read().await.len(), 0);
}

#[tokio::test]
async fn test_sprint4_embedding_cache_ttl() {
    use std::time::{Instant, Duration};

    // 1. Manually populate the cache with one fresh and one expired entry
    let mut cache = EMBEDDING_CACHE.write().await;
    cache.clear();

    cache.insert(
        "fresh".to_string(),
        EmbeddingCacheEntry {
            vector: vec![1.0, 2.0, 3.0],
            cached_at: Instant::now(),
        },
    );

    cache.insert(
        "expired".to_string(),
        EmbeddingCacheEntry {
            vector: vec![4.0, 5.0, 6.0],
            // Simulate an entry that is older than EMBEDDING_CACHE_TTL_SECS (3600s)
            cached_at: Instant::now() - Duration::from_secs(4000),
        },
    );
    assert_eq!(cache.len(), 2);
    drop(cache);

    // 2. Run the cleaner
    xavier::memory::qmd_memory::reader::clean_embedding_cache().await;

    // 3. Verify eviction
    let cache = EMBEDDING_CACHE.read().await;
    assert_eq!(cache.len(), 1);
    assert!(cache.contains_key("fresh"));
    assert!(!cache.contains_key("expired"));
}

#[tokio::test]
async fn test_backend_persistence_init() {
    // 1. Setup a persistent store (InMemory is fine for logic testing as long as we keep the instance)
    let store = Arc::new(InMemoryMemoryStore::new());
    let workspace_id = "test-persistence";

    // 2. Add a record to the store
    let record = MemoryRecord {
        id: "persisted-1".to_string(),
        workspace_id: workspace_id.to_string(),
        path: "important/data".to_string(),
        content: "this must survive init".to_string(),
        primary: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        ..Default::default()
    };
    store.put(record).await.expect("put failed");

    // 3. Initialize a NEW QmdMemory instance over the SAME store
    let docs = Arc::new(AsyncRwLock::new(Vec::new()));
    let memory = QmdMemory::new_with_workspace(docs.clone(), workspace_id);
    memory.set_store(store as Arc<dyn MemoryStore>).await;

    // 4. Call init() - this should load data from the store into 'docs'
    memory.init().await.expect("init failed");

    // 5. Verify data is loaded
    let loaded_docs = docs.read().await;
    assert_eq!(loaded_docs.len(), 1);
    assert_eq!(loaded_docs[0].path, "important/data");
    assert_eq!(loaded_docs[0].content, "this must survive init");
}
