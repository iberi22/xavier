//! Integration tests for memory prune policy and consolidation in `VecSqliteMemoryStore` and V1 API.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    routing::{get, post},
    Extension, Router,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;
use ulid::Ulid;

use xavier::{
    agents::RuntimeConfig,
    app::security_service::SecurityService,
    memory::{
        agent_indexer::AgentIndexer,
        file_indexer::{FileIndexer, FileIndexerConfig},
        schema::MemoryLevel,
        sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig},
        store::{MemoryBackend, MemoryRecord, MemoryStore},
    },
    server::v1_api,
    settings::types::{DedupScope, DedupSettings},
    workspace::{
        EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext,
        WorkspaceRegistry, WorkspaceState,
    },
    AppState,
};

fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should not be before UNIX epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique}-{suffix}"))
}

async fn create_test_app() -> (Router, WorkspaceContext) {
    let unique_id = Ulid::new().to_string();
    let db_path = unique_test_path(&format!("prune-test-{}", unique_id), "code_graph.db");
    let code_db = Arc::new(
        code_graph::db::CodeGraphDB::new(&db_path)
            .expect("failed to create CodeGraphDB for prune test"),
    );
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let workspace_registry = Arc::new(WorkspaceRegistry::new());

    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: format!("test-prune-{}", unique_id),
            token: format!("test-prune-token-{}", unique_id),
            plan: PlanTier::Personal,
            memory_backend: MemoryBackend::Memory,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::CloudMirror,
            dedup: DedupSettings::default(),
        },
        RuntimeConfig::default(),
        unique_test_path(&format!("prune-panel-{}", unique_id), "threads"),
    )
    .await
    .expect("failed to create WorkspaceState for prune test");

    workspace_registry
        .insert(workspace)
        .await
        .expect("failed to insert workspace into registry");
    let workspace_context = workspace_registry
        .authenticate(&format!("test-prune-token-{}", unique_id))
        .await
        .expect("failed to authenticate with test token");

    let state = AppState {
        workspace_registry,
        indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
        agent_indexer: AgentIndexer::new(FileIndexer::new(
            FileIndexerConfig::default(),
            Some(code_indexer.clone()),
        )),
        code_indexer,
        code_query,
        code_db,
        security_service: Arc::new(SecurityService::new()),
        code_graph_dump_path: None,
    };

    let router = Router::new()
        .route(
            "/v1/memories",
            post(v1_api::v1_memories_add).get(v1_api::v1_memories_list),
        )
        .route(
            "/v1/memories/{id}",
            get(v1_api::v1_memories_get).delete(v1_api::v1_memories_delete),
        )
        .route("/v1/memories/prune", post(v1_api::v1_memories_prune))
        .layer(Extension(workspace_context.clone()))
        .with_state(state);

    (router, workspace_context)
}

#[tokio::test]
async fn test_prune_dry_run_does_not_delete() {
    let (app, _context) = create_test_app().await;

    // Add memories eligible for prune
    let add_req1 = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "Temporary task entry 1",
                "user_id": "tmp_task_1",
                "kind": "task",
                "metadata": { "kind": "task" }
            })
            .to_string(),
        ))
        .expect("build add req1");
    let resp1 = app.clone().oneshot(add_req1).await.expect("exec add req1");
    assert_eq!(resp1.status(), StatusCode::OK);

    let add_req2 = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "Temporary task entry 2",
                "user_id": "tmp_task_2",
                "kind": "task",
                "metadata": { "kind": "task" }
            })
            .to_string(),
        ))
        .expect("build add req2");
    let resp2 = app.clone().oneshot(add_req2).await.expect("exec add req2");
    assert_eq!(resp2.status(), StatusCode::OK);

    // Prune with dry_run = true (explicit)
    let prune_req = Request::builder()
        .method("POST")
        .uri("/v1/memories/prune")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "kind": "task",
                "dry_run": true
            })
            .to_string(),
        ))
        .expect("build prune dry_run req");

    let resp_prune = app.clone().oneshot(prune_req).await.expect("exec prune dry_run req");
    assert_eq!(resp_prune.status(), StatusCode::OK);

    let body = to_bytes(resp_prune.into_body(), usize::MAX)
        .await
        .expect("read prune dry_run body");
    let val: serde_json::Value = serde_json::from_slice(&body).expect("parse prune JSON");

    assert_eq!(val["status"], "ok");
    assert_eq!(val["matched"], 2);
    assert_eq!(val["deleted"], 0);
    assert_eq!(val["dry_run"], true);

    // Verify memories were NOT deleted from the store
    let list_req = Request::builder()
        .method("GET")
        .uri("/v1/memories")
        .body(Body::empty())
        .expect("build list req");
    let resp_list = app.oneshot(list_req).await.expect("exec list req");
    let body_list = to_bytes(resp_list.into_body(), usize::MAX)
        .await
        .expect("read list body");
    let list_val: serde_json::Value = serde_json::from_slice(&body_list).expect("parse list JSON");

    assert_eq!(list_val["memories"].as_array().expect("array").len(), 2);
}

#[tokio::test]
async fn test_prune_older_than_days_removes_only_old_records() {
    let (app, _context) = create_test_app().await;

    // Add old memory (last_accessed_at set to 10 days ago)
    let old_access_time = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
    let add_old = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "Stale cached result",
                "user_id": "cache_old",
                "kind": "fact",
                "metadata": {
                    "kind": "fact",
                    "last_accessed_at": old_access_time
                }
            })
            .to_string(),
        ))
        .expect("build add old");
    let resp_old = app.clone().oneshot(add_old).await.expect("exec add old");
    assert_eq!(resp_old.status(), StatusCode::OK);

    // Add recent memory (last_accessed_at current)
    let add_new = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "Fresh active cache entry",
                "user_id": "cache_new",
                "kind": "fact",
                "metadata": {
                    "kind": "fact"
                }
            })
            .to_string(),
        ))
        .expect("build add new");
    let resp_new = app.clone().oneshot(add_new).await.expect("exec add new");
    assert_eq!(resp_new.status(), StatusCode::OK);

    // Execute prune with older_than_days = 1 and dry_run = false
    let prune_req = Request::builder()
        .method("POST")
        .uri("/v1/memories/prune")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "older_than_days": 1,
                "dry_run": false
            })
            .to_string(),
        ))
        .expect("build prune older_than_days req");

    let resp_prune = app.clone().oneshot(prune_req).await.expect("exec prune req");
    assert_eq!(resp_prune.status(), StatusCode::OK);

    let body = to_bytes(resp_prune.into_body(), usize::MAX)
        .await
        .expect("read prune body");
    let val: serde_json::Value = serde_json::from_slice(&body).expect("parse prune JSON");

    assert_eq!(val["status"], "ok");
    assert_eq!(val["matched"], 1);
    assert_eq!(val["deleted"], 1);
    assert_eq!(val["dry_run"], false);

    // Verify only cache_new remains
    let list_req = Request::builder()
        .method("GET")
        .uri("/v1/memories")
        .body(Body::empty())
        .expect("build list req");
    let resp_list = app.oneshot(list_req).await.expect("exec list req");
    let body_list = to_bytes(resp_list.into_body(), usize::MAX)
        .await
        .expect("read list body");
    let list_val: serde_json::Value = serde_json::from_slice(&body_list).expect("parse list JSON");

    let memories = list_val["memories"].as_array().expect("array");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0]["user_id"].as_str(), Some("cache_new"));
}

#[tokio::test]
async fn test_prune_kind_and_path_prefix_filtering() {
    let (app, _context) = create_test_app().await;

    // Add memory 1: path_prefix "logs/temp", kind "fact"
    let add1 = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "Fact record 1",
                "path": "logs/temp/1",
                "kind": "fact",
                "metadata": { "kind": "fact" }
            })
            .to_string(),
        ))
        .expect("build add1");
    app.clone().oneshot(add1).await.unwrap();

    // Add memory 2: path_prefix "logs/temp", kind "decision"
    let add2 = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "Decision record 1",
                "path": "logs/temp/2",
                "kind": "decision",
                "metadata": { "kind": "decision" }
            })
            .to_string(),
        ))
        .expect("build add2");
    app.clone().oneshot(add2).await.unwrap();

    // Add memory 3: path_prefix "system/perm", kind "fact"
    let add3 = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "text": "System fact record",
                "path": "system/perm/1",
                "kind": "fact",
                "metadata": { "kind": "fact" }
            })
            .to_string(),
        ))
        .expect("build add3");
    app.clone().oneshot(add3).await.unwrap();

    // Prune with path_prefix "logs/temp" AND kind "fact"
    let prune_req = Request::builder()
        .method("POST")
        .uri("/v1/memories/prune")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "path_prefix": "logs/temp",
                "kind": "fact",
                "dry_run": false
            })
            .to_string(),
        ))
        .expect("build prune req");

    let resp_prune = app.clone().oneshot(prune_req).await.unwrap();
    let body = to_bytes(resp_prune.into_body(), usize::MAX).await.unwrap();
    let val: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(val["matched"], 1);
    assert_eq!(val["deleted"], 1);

    // Remaining should be memory 2 (decision) and memory 3 (system/perm/1)
    let list_req = Request::builder()
        .method("GET")
        .uri("/v1/memories")
        .body(Body::empty())
        .expect("list req");
    let resp_list = app.oneshot(list_req).await.unwrap();
    let body_list = to_bytes(resp_list.into_body(), usize::MAX).await.unwrap();
    let list_val: serde_json::Value = serde_json::from_slice(&body_list).unwrap();
    let memories = list_val["memories"].as_array().unwrap();
    assert_eq!(memories.len(), 2);
}

#[tokio::test]
async fn test_consolidation_merge_duplicate_paths_after_dedup_config_change() {
    let db_path = unique_test_path("vec_store_consolidation", "test.db");
    let config = VecSqliteStoreConfig {
        path: db_path,
        embedding_dimensions: 4,
    };

    let store = VecSqliteMemoryStore::new(config).await.expect("initialize store");
    let workspace_id = "ws_dedup_test";

    // 1. Initial configuration: dedup disabled
    let default_settings = DedupSettings {
        enabled: false,
        threshold: 0.85,
        scope: DedupScope::PathExact,
        max_revisions: 5,
    };
    store.set_dedup_settings(default_settings).await;

    let rec1 = MemoryRecord {
        id: "rec_1".to_string(),
        workspace_id: workspace_id.to_string(),
        path: "doc/knowledge".to_string(),
        content: "Knowledge base entry regarding Xavier core features".to_string(),
        metadata: serde_json::json!({ "dedup": true }),
        embedding: vec![0.1, 0.2, 0.3, 0.4],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: vec![],
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
        score: 0.0,
        deleted_at: None,
        ..Default::default()
    };

    store.put(rec1.clone()).await.expect("put initial record");

    let items_before = store.list(workspace_id).await.expect("list before dedup");
    assert_eq!(items_before.len(), 1);

    // 2. Change dedup configuration: enable deduplication with PathExact scope
    let active_settings = DedupSettings {
        enabled: true,
        threshold: 0.80,
        scope: DedupScope::PathExact,
        max_revisions: 5,
    };
    store.set_dedup_settings(active_settings).await;

    // Put a new record with superset content on the exact same path with similar vector
    let rec2 = MemoryRecord {
        id: "rec_2".to_string(),
        workspace_id: workspace_id.to_string(),
        path: "doc/knowledge".to_string(),
        content: "Knowledge base entry regarding Xavier core features with consolidated updates".to_string(),
        metadata: serde_json::json!({ "dedup": true }),
        embedding: vec![0.11, 0.21, 0.31, 0.41],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        revision: 1,
        primary: true,
        parent_id: None,
        cluster_id: None,
        level: MemoryLevel::Raw,
        relation: None,
        clearance: Default::default(),
        revisions: vec![],
        encrypted_dek: None,
        content_iv: None,
        metadata_iv: None,
        score: 0.0,
        deleted_at: None,
        ..Default::default()
    };

    store.put(rec2).await.expect("put superset record with dedup active");

    // The store should merge into the existing record rather than duplicate rows
    let items_after = store.list(workspace_id).await.expect("list after dedup");
    assert_eq!(items_after.len(), 1, "Duplicate paths should consolidate into a single record");
    assert_eq!(items_after[0].id, "rec_1");
    assert!(items_after[0].content.contains("consolidated updates"));
}
