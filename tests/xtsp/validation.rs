//! XTSP end-to-end integration flow tests
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    routing::{get, post},
    Extension, Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

use xavier::server::v1_api::{
    v1_memories_add, v1_memories_delete, v1_memories_get, v1_memories_list, v1_memories_prune,
    v1_memories_search, v1_memories_update,
};
use xavier::workspace::WorkspaceContext;
use xavier::AppState;

static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX_EPOCH")
        .as_nanos();
    // Add thread ID to avoid collisions between concurrent tests
    let tid = std::thread::current().id();
    std::env::temp_dir().join(format!("{prefix}-{unique:016x}-{tid:?}-{suffix}"))
}

/// Test state.
pub async fn test_state() -> (AppState, WorkspaceContext, mockito::ServerGuard) {
    let mut server = mockito::Server::new_async().await;
    let mock_url = server.url();

    let vec_db_path = unique_test_path("xavier-vec-store", "memories_vec.db");
    std::env::set_var(
        "XAVIER_MEMORY_VEC_PATH",
        vec_db_path.to_string_lossy().to_string(),
    );
    std::env::set_var("XAVIER_EMBEDDING_DIMENSIONS", "1536");
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
    std::env::set_var(
        "XAVIER_EMBEDDING_URL",
        format!("{}/v1/embeddings", mock_url),
    );
    std::env::set_var("OPENAI_API_KEY", "sk-mock-key");
    std::env::set_var("XAVIER_EMBEDDING_MODEL", "text-embedding-3-small");

    // Mock models list for auto-probing if needed
    let _mock_models = server
        .mock("GET", "/v1/models")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
                "object": "list",
                "data": [
                    {
                        "id": "text-embedding-3-small",
                        "object": "model"
                    }
                ]
            }"#,
        )
        .create_async()
        .await;

    // Mock embeddings generation
    let mock_vector = vec![0.1f32; 1536];
    let mock_body = serde_json::json!({
        "data": [
            {
                "embedding": mock_vector
            }
        ]
    });
    let _mock_embeddings = server
        .mock("POST", "/v1/embeddings")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(serde_json::to_string(&mock_body).unwrap())
        .create_async()
        .await;

    std::env::set_var("XAVIER_TOKEN", "test-token");
    let db_path = unique_test_path("xavier-code-mcp", "code_graph.db");
    let code_db = Arc::new(
        code_graph::db::CodeGraphDB::new(&db_path).expect("CodeGraphDB creation failed for test"),
    );
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let workspace_registry = Arc::new(xavier::workspace::WorkspaceRegistry::new());
    let workspace = xavier::workspace::WorkspaceState::new(
        xavier::workspace::WorkspaceConfig {
            id: format!("test-{}", ulid::Ulid::new()),
            token: "test-token".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::Vec, // SQLite-Vec backend
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            dedup: xavier::settings::types::DedupSettings {
                enabled: true,
                ..xavier::settings::types::DedupSettings::default()
            },
        },
        xavier::agents::RuntimeConfig::default(),
        unique_test_path("xavier-mcp-store", "threads"),
    )
    .await
    .expect("WorkspaceState creation failed for test");
    workspace_registry
        .insert(workspace)
        .await
        .expect("insert workspace into registry failed");
    let workspace = workspace_registry
        .authenticate("test-token")
        .await
        .expect("authenticate failed for test workspace");

    (
        AppState {
            workspace_registry,
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
            security_service: Arc::new(xavier::app::security_service::SecurityService::new()),
            code_graph_dump_path: None,
        },
        workspace,
        server,
    )
}

/// Test router.
pub fn test_router(state: AppState, workspace: WorkspaceContext) -> Router {
    Router::new()
        .route("/mcp", post(xavier::server::mcp::mcp_post_handler))
        .layer(axum::middleware::from_fn(
            xavier::server::mcp::auth::mcp_auth_middleware,
        ))
        .layer(axum::Extension(workspace))
        .with_state(state)
}

/// Post json.
pub async fn post_json(app: Router, body: Value) -> axum::response::Response {
    let method = body
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("initialize");

    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("Origin", "http://localhost:8080")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", method);

    // Add Mcp-Name header for requests that require it (tools/call, resources/read, prompts/get)
    if method == "tools/call" {
        if let Some(name) = body
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        {
            req = req.header("Mcp-Name", name);
        }
    } else if method == "resources/read" {
        if let Some(uri) = body
            .get("params")
            .and_then(|p| p.get("uri"))
            .and_then(|n| n.as_str())
        {
            req = req.header("Mcp-Name", uri);
        }
    }

    if let Ok(t) = std::env::var("XAVIER_TOKEN") {
        req = req.header("X-Xavier-Token", t);
    }

    app.oneshot(
        req.body(Body::from(
            serde_json::to_vec(&body).expect("serialize json body failed"),
        ))
        .expect("build POST request failed"),
    )
    .await
    .expect("POST request to MCP endpoint failed")
}

/// Get json body.
pub async fn get_json_body(response: axum::response::Response) -> Value {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body failed");
    serde_json::from_slice(&body_bytes).expect("parse JSON response body failed")
}

/// Create a V1 API Router in-process.
pub fn v1_router(state: AppState, workspace: WorkspaceContext) -> Router {
    Router::new()
        .route("/v1/memories", post(v1_memories_add).get(v1_memories_list))
        .route(
            "/v1/memories/{id}",
            get(v1_memories_get)
                .put(v1_memories_update)
                .delete(v1_memories_delete),
        )
        .route("/v1/memories/search", post(v1_memories_search))
        .route("/v1/memories/prune", post(v1_memories_prune))
        .layer(Extension(workspace))
        .with_state(state)
}

/// Helper to execute a POST request to V1 API.
async fn post_v1_json(app: Router, uri: &str, body: Value) -> axum::response::Response {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Helper to execute a GET request to V1 API.
async fn get_v1(app: Router, uri: &str) -> axum::response::Response {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Helper to execute a PUT request to V1 API.
async fn put_v1_json(app: Router, uri: &str, body: Value) -> axum::response::Response {
    let req = Request::builder()
        .method(Method::PUT)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    app.oneshot(req).await.unwrap()
}

/// Reads the response body and parses it as Serde Value.
async fn read_v1_json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn xtsp_fat_search() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // Seed memory via V1 memories add
    let long_content = "This is a very long memory document with a significant amount of text. It should definitely exceed 100 characters in length to ensure snippet truncations are clearly distinct and useful for clients.";
    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": long_content,
            "user_id": "xtsp-user-1",
            "metadata": {"category": "test"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    // Perform search in snippet mode
    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "significant",
            "mode": "snippet",
            "limit": 5
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);

    let body = read_v1_json_body(search_res).await;
    assert_eq!(body["mode"], "snippet");

    let results = body["results"].as_array().expect("results should be array");
    assert!(!results.is_empty(), "search results should not be empty");

    for result in results {
        assert!(result.get("id").is_some());
        assert!(result.get("snippet").is_some());
        assert!(result.get("score").is_some());
        assert!(result.get("path").is_some());
        assert!(result.get("kind").is_some());
        // Verify no full content field
        assert!(result.get("memory").is_none());
        assert!(result.get("content").is_none());
        assert!(result.get("embedding").is_none());

        let snippet = result["snippet"].as_str().unwrap();
        assert!(snippet.len() <= 100);
    }
}

#[tokio::test]
async fn xtsp_page_in() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let mcp_app = test_router(state, workspace);

    // Create memory via MCP create_memory tool
    let add_res = post_json(
        mcp_app.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "create_memory",
                "arguments": {
                    "path": "protocols/xtsp-spec",
                    "content": "The XTSP protocol standardizes fat index search with progressive disclosure and page-in mechanism."
                }
            }
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    // Call mem_search via MCP
    let search_res = post_json(
        mcp_app.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "mem_search",
                "arguments": {
                    "query": "progressive disclosure",
                    "include_content": false
                }
            }
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);

    let search_body = get_json_body(search_res).await;
    let candidates_val = &search_body["result"]["content"][0]["structuredContent"]["candidates"];
    let candidates = candidates_val
        .as_array()
        .expect("candidates should be array");
    assert!(!candidates.is_empty(), "candidates should not be empty");

    let candidate = &candidates[0];
    let path = candidate["path"].as_str().expect("path should be string");
    // Verify candidate does not contain full content
    assert!(candidate.get("content").is_none());

    // Page-in full content using memory_context tool using path (which gets correctly resolved by fallback)
    let context_res = post_json(
        mcp_app.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "memory_context",
                "arguments": {
                    "ids": [path],
                    "depth": 0
                }
            }
        }),
    )
    .await;
    assert_eq!(context_res.status(), StatusCode::OK);

    let context_body = get_json_body(context_res).await;
    let context_data = &context_body["result"]["content"][0]["structuredContent"];
    let content = context_data["content"]
        .as_str()
        .expect("content should be string");
    assert!(
        content.contains("The XTSP protocol standardizes fat index search"),
        "context should contain original content"
    );
}

#[tokio::test]
async fn xtsp_persist() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // POST /v1/memories stores a document
    let text = "XTSP persistence test text representing stored memory.";
    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": text,
            "user_id": "test-user-persist",
            "metadata": {"type": "test"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    let add_body = read_v1_json_body(add_res).await;
    let id = add_body["id"].as_str().expect("id should be string");

    // Retrieve by GET
    let get_res = get_v1(app.clone(), &format!("/v1/memories/{}", id)).await;
    assert_eq!(get_res.status(), StatusCode::OK);
    let get_body = read_v1_json_body(get_res).await;
    assert_eq!(get_body["memory"]["memory"].as_str().unwrap(), text);

    // Retrieve by search
    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "persistence",
            "limit": 5
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);
    let search_body = read_v1_json_body(search_res).await;
    let results = search_body["results"]
        .as_array()
        .expect("results should be array");
    let found = results
        .iter()
        .any(|item| item["id"].as_str().unwrap() == id && item["memory"].as_str().unwrap() == text);
    assert!(found, "stored memory should be retrievable by search");
}

#[tokio::test]
async fn xtsp_dedup() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state.clone(), workspace.clone());

    let text = "Identical content for dedup verification.";
    let user_id = "test-user-dedup";

    println!("DEDUP DEBUG: Starting 3 writes...");
    // Write three identical documents with mode=dedup
    for i in 0..3 {
        let add_res = post_v1_json(
            app.clone(),
            "/v1/memories?mode=dedup",
            json!({
                "text": text,
                "user_id": user_id,
                "metadata": {"type": "dedup-test"}
            }),
        )
        .await;
        assert_eq!(add_res.status(), StatusCode::OK);
        let add_body = read_v1_json_body(add_res).await;
        println!("DEDUP DEBUG: Write {} status: {:?}", i, add_body);
    }

    // Re-sync/initialize the memory cache from the persistent store
    workspace.workspace.memory.init().await.unwrap();

    // List all memories in storage directly to inspect
    let docs = workspace.workspace.memory.all_documents().await;
    println!("DEDUP DEBUG: Direct memory document count: {}", docs.len());
    for (i, doc) in docs.iter().enumerate() {
        println!(
            "DEDUP DEBUG: Direct doc {}: id={:?}, path={}, content_vector_len={:?}",
            i,
            doc.id,
            doc.path,
            doc.content_vector.as_ref().map(|v| v.len())
        );
    }

    // List all memories
    let list_res = get_v1(app.clone(), "/v1/memories?limit=100").await;
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_body = read_v1_json_body(list_res).await;
    let memories = list_body["memories"]
        .as_array()
        .expect("memories should be array");

    // Should only have exactly 1 memory
    assert_eq!(
        memories.len(),
        1,
        "There should be exactly one memory after 3 identical writes"
    );
    assert_eq!(memories[0]["memory"].as_str().unwrap(), text);
}

#[tokio::test]
async fn xtsp_prune() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // Seed multiple documents
    let add_res1 = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": "Stale draft memory.",
            "user_id": "prune-prefix/doc1",
            "metadata": {"kind": "decision"}
        }),
    )
    .await;
    assert_eq!(add_res1.status(), StatusCode::OK);

    let add_res2 = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": "Another stale draft memory.",
            "user_id": "prune-prefix/doc2",
            "metadata": {"kind": "decision"}
        }),
    )
    .await;
    assert_eq!(add_res2.status(), StatusCode::OK);

    // dry_run=true returns count but deletes nothing
    let prune_dry_res = post_v1_json(
        app.clone(),
        "/v1/memories/prune",
        json!({
            "path_prefix": "prune-prefix/",
            "dry_run": true
        }),
    )
    .await;
    assert_eq!(prune_dry_res.status(), StatusCode::OK);
    let prune_dry_body = read_v1_json_body(prune_dry_res).await;
    assert_eq!(prune_dry_body["matched"].as_u64().unwrap(), 2);
    assert_eq!(prune_dry_body["deleted"].as_u64().unwrap(), 0);
    assert!(prune_dry_body["dry_run"].as_bool().unwrap());

    // Check that memories are still there
    let list_res_before = get_v1(app.clone(), "/v1/memories?limit=10").await;
    let list_body_before = read_v1_json_body(list_res_before).await;
    assert_eq!(list_body_before["memories"].as_array().unwrap().len(), 2);

    // actual run (dry_run=false) deletes
    let prune_act_res = post_v1_json(
        app.clone(),
        "/v1/memories/prune",
        json!({
            "path_prefix": "prune-prefix/",
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(prune_act_res.status(), StatusCode::OK);
    let prune_act_body = read_v1_json_body(prune_act_res).await;
    assert_eq!(prune_act_body["matched"].as_u64().unwrap(), 2);
    assert_eq!(prune_act_body["deleted"].as_u64().unwrap(), 2);
    assert!(!prune_act_body["dry_run"].as_bool().unwrap());

    // Verify memories are deleted
    let list_res_after = get_v1(app.clone(), "/v1/memories?limit=10").await;
    let list_body_after = read_v1_json_body(list_res_after).await;
    assert_eq!(list_body_after["memories"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn xtsp_full_flow() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // Seed original document
    let original_text = "Original memory body for full flow test.";
    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": original_text,
            "user_id": "flow-user",
            "metadata": {"type": "flow"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);
    let add_body = read_v1_json_body(add_res).await;
    let id = add_body["id"].as_str().expect("id should be string");

    // 1. Fat Search: search with mode=snippet to get ID without full text
    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "Original memory",
            "mode": "snippet"
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);
    let search_body = read_v1_json_body(search_res).await;
    let search_results = search_body["results"]
        .as_array()
        .expect("results should be array");
    assert!(!search_results.is_empty());
    let search_item = &search_results[0];
    let found_id = search_item["id"].as_str().expect("id should be string");
    assert_eq!(found_id, id);
    assert!(search_item.get("memory").is_none());

    // 2. Page-In: use ID to retrieve full original content
    let get_res = get_v1(app.clone(), &format!("/v1/memories/{}", found_id)).await;
    assert_eq!(get_res.status(), StatusCode::OK);
    let get_body = read_v1_json_body(get_res).await;
    let paged_text = get_body["memory"]["memory"]
        .as_str()
        .expect("memory should be string");
    assert_eq!(paged_text, original_text);

    // 3. Modify -> 4. Persist (PUT update)
    let modified_text = "Modified memory body for full flow test.";
    let update_res = put_v1_json(
        app.clone(),
        &format!("/v1/memories/{}", found_id),
        json!({
            "text": modified_text,
            "user_id": "flow-user"
        }),
    )
    .await;
    assert_eq!(update_res.status(), StatusCode::OK);

    // 5. Verify the update
    let get_verify_res = get_v1(app.clone(), &format!("/v1/memories/{}", found_id)).await;
    assert_eq!(get_verify_res.status(), StatusCode::OK);
    let get_verify_body = read_v1_json_body(get_verify_res).await;
    let final_text = get_verify_body["memory"]["memory"]
        .as_str()
        .expect("memory should be string");
    assert_eq!(final_text, modified_text);
}

#[tokio::test]
async fn xtsp_token_savings() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // Seed very large document to make token savings extremely obvious using realistic markdown
    let mut large_markdown = String::new();
    large_markdown.push_str("# Xavier Cognitive Memory Architecture Spec\n\n");
    large_markdown.push_str("## Executive Summary\n\n");
    large_markdown.push_str("Xavier is a next-generation high-fidelity decentralized memory agent system designed for high autonomous throughput, advanced privacy-preserving access control, and semantic density. This spec provides a thorough and comprehensive overview of the design principles, operational mechanics, tokenomics models, and the benchmark criteria defining Xavier's development lifecycle.\n\n");

    large_markdown.push_str("## 1. Core Architecture Overview\n\n");
    large_markdown.push_str("At the core of Xavier is a Hexagonal Architecture (Ports and Adapters design pattern), separating the core domain workflows from transport protocols, external persistence adapters, and cryptographic providers. This separation of concerns ensures that the core codebase remains highly maintainable and easily testable without mock-heavy or slow integration pipelines.\n\n");
    large_markdown
        .push_str("The architecture is organized into distinct, well-defined layers:\n\n");
    large_markdown.push_str("- **L0: Semantic Vector Cache**: Extremely fast local memory backed by memory-mapped SQLite-Vec extensions.\n");
    large_markdown.push_str("- **L1: Knowledge Graph (Belief System)**: Maps named entities and complex conceptual relations using a lightweight in-memory directed acyclic graph.\n");
    large_markdown.push_str("- **L2: Episodic Store**: Keeps chronological, context-aware session transcripts synchronized periodically.\n\n");

    large_markdown.push_str("## 2. Advanced Memory Snippeting and Query-Aware Centering\n\n");
    large_markdown.push_str("Xavier implements high-fidelity query-aware snippet extraction. Rather than blindly returning the first N characters of a document (which might be irrelevant frontmatter or table headers), the snippet engine dynamically locates the occurrence of query terms inside the body of the memory and generates a centered excerpt. This optimization drastically improves downstream context density and delivers substantial token savings for large language models (LLMs).\n\n");

    large_markdown.push_str("```rust\n");
    large_markdown.push_str("pub fn extract_query_centered_excerpt(content: &str, query: &str, budget: usize) -> String {\n");
    large_markdown.push_str("    let (body, _) = strip_frontmatter(content);\n");
    large_markdown.push_str("    let window = find_matching_window(body, query);\n");
    large_markdown.push_str("    clip_chars_around_window(body, window, budget)\n");
    large_markdown.push_str("}\n");
    large_markdown.push_str("```\n\n");

    large_markdown
        .push_str("## 3. High-Throughput Token Staking and Node Activation Mechanics\n\n");
    large_markdown.push_str("Integrating central Web2 billing providers like Stripe is strictly prohibited under the project guidelines; all network locking, node activation, and incentives rely solely on utility token staking ($SWAL) or node-level mesh treasury ownership. Nodes operating within the Xavier Mesh network are required to register themselves by staking a minimum threshold of tokens to guarantee high performance, low-latency execution, and honest security reporting.\n\n");

    for i in 1..=2 {
        large_markdown.push_str(&format!(
            "### Subsection 3.{} - Distributed Ledger Staking Phase {}\n\n",
            i, i
        ));
        large_markdown.push_str("Distributed state consistency is maintained across all network participants via a lightweight consensus loop. ");
        large_markdown.push_str("When a node joins, it executes a pair of handshake protocols to prove its local capacity and stake integrity. ");
        large_markdown.push_str("Staking transactions are recorded in the local mesh treasury using multi-signature wallets to prevent double-spending or single-point-of-failure vulnerabilities. ");
        large_markdown.push_str("This staking mechanism enforces economic alignment without central web2 middle-men, guaranteeing complete data sovereignty.\n\n");
    }

    large_markdown.push_str("## 4. Evaluation and Benchmarking Criteria\n\n");
    large_markdown.push_str("The following table illustrates the performance benchmarks required for production deployment:\n\n");
    large_markdown.push_str("| Metric ID | Description | Threshold | Target | Status |\n");
    large_markdown.push_str("|-----------|-------------|-----------|--------|--------|\n");
    large_markdown.push_str("| M-101     | Latency     | < 50ms    | < 15ms | Stable |\n");
    large_markdown.push_str("| M-102     | Recall@10   | > 85%     | > 95%  | Stable |\n");
    large_markdown.push_str("| M-103     | Precision@5 | > 90%     | > 98%  | Stable |\n");
    large_markdown.push_str("| M-104     | Compression | > 5x      | > 10x  | Stable |\n\n");

    large_markdown.push_str("## 5. Security & Threat Modeling\n\n");
    large_markdown.push_str("Security is enforced at every layer of the incoming data stream. The SecurityService runs multi-layered scanning engines to detect prompt injection vectors, high-entropy API key leaks, and unauthorized mesh command injections. ");
    large_markdown.push_str(
        "If a potential threat is detected, the request is immediately quarantined and logged. ",
    );
    large_markdown.push_str("This robust security policy ensures that the node is protected against malicious agents and adversarial attacks while maintaining low overhead and zero-copy performance metrics.\n\n");

    large_markdown.push_str("## 6. Long-Term Roadmap and Future Milestones\n\n");
    large_markdown.push_str("Our long-term roadmap focuses on continuous performance optimization, zero-dependency local mathematical simulation models, and the integration of highly specialized local LLM sidecars. By avoiding standard Web2 APIs, Xavier remains fully resilient against external service outages, pricing changes, or centralized censorship. ");
    large_markdown.push_str("The decentralized marketplace allows nodes to dynamically trade datasets, share compute capacity, and participate in governance DAO proposals on-chain.\n\n");

    for section in 1..=1 {
        large_markdown.push_str(&format!(
            "## Section Appendix {} - Detailed Technical Implementation Notes\n\n",
            section
        ));
        large_markdown.push_str("This appendix contains supplementary information detailing the dynamic Reciprocal Rank Fusion parameters. ");
        large_markdown.push_str("To balance performance and accuracy, Xavier adjusts the RRF K value based on the underlying dataset size. ");
        large_markdown.push_str(
            "For tiny datasets under 100 entries, K is optimized to 10 to ensure swift recall. ",
        );
        large_markdown.push_str("For medium datasets (up to 500 entries), K is scaled to 30. ");
        large_markdown.push_str("Large corpora utilize K=60. ");
        large_markdown.push_str("This adaptive scaling mechanism minimizes memory overhead while preserving the high precision of search queries.\n\n");

        large_markdown.push_str("Additionally, we document the FTS5 triggers that synchronize the virtual FTS database automatically when any memory is added, updated, or deleted. ");
        large_markdown.push_str("This completely eliminates the need for manual indexing jobs, providing a completely hands-off experience for node operators. ");
        large_markdown.push_str("We also ensure that all string truncations are strictly bounded using the UTF-8 safe `clip_chars` utility to prevent index out of bounds panics.\n\n");
    }

    assert!(large_markdown.len() > 5000);
    assert!(large_markdown.len() < 7000);

    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": large_markdown,
            "user_id": "token-test-user-v2",
            "metadata": {"category": "benchmark-markdown"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    // Perform Full Search (normal)
    let full_search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "Staking Phase",
            "limit": 1
        }),
    )
    .await;
    assert_eq!(full_search_res.status(), StatusCode::OK);
    let full_response_bytes = axum::body::to_bytes(full_search_res.into_body(), usize::MAX)
        .await
        .unwrap();
    let full_response_str = String::from_utf8(full_response_bytes.to_vec()).unwrap();

    // Perform Snippet Search (compact)
    let snippet_search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "Staking Phase",
            "mode": "snippet",
            "limit": 1
        }),
    )
    .await;
    assert_eq!(snippet_search_res.status(), StatusCode::OK);
    let snippet_response_bytes = axum::body::to_bytes(snippet_search_res.into_body(), usize::MAX)
        .await
        .unwrap();
    let snippet_response_str = String::from_utf8(snippet_response_bytes.to_vec()).unwrap();

    // Honest token estimation: char_count / 4
    let full_response_size = full_response_str.chars().count() as f64 / 4.0;
    let snippet_response_size = snippet_response_str.chars().count() as f64 / 4.0;

    println!("Full response string: {}", full_response_str);
    println!("Snippet response string: {}", snippet_response_str);
    println!(
        "Full response size (honest token estimation): {}",
        full_response_size
    );
    println!(
        "Snippet response size (honest token estimation): {}",
        snippet_response_size
    );

    assert!(
        snippet_response_size < 0.15 * full_response_size,
        "Snippet response size ({}) is not < 15% of full response size ({})",
        snippet_response_size,
        full_response_size
    );
}

#[tokio::test]
async fn xtsp_snippet_skips_frontmatter() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    let content_with_fm = r#"---
title: "Technical Spec Doc"
author: "Xavier Team"
project: "XTSP"
---
This is actual body content starting here. We are implementing the progressive disclosure mechanism."#;

    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": content_with_fm,
            "user_id": "fm-test-user",
            "metadata": {"category": "frontmatter-test"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "disclosure",
            "mode": "snippet"
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);
    let body = read_v1_json_body(search_res).await;
    let results = body["results"].as_array().expect("results should be array");
    assert!(!results.is_empty());

    let snippet = results[0]["snippet"].as_str().unwrap();
    assert!(!snippet.contains("---"));
    assert!(!snippet.contains("Technical Spec Doc"));
    assert!(!snippet.contains("author:"));
    assert!(snippet.contains("actual body content"));
}

#[tokio::test]
async fn xtsp_snippet_centers_on_query() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    let long_text = "This is a very long text at the beginning. It goes on and on for a while to make sure we have a lot of prefix padding. Finally, we reach the special target query parameter. Then we have a very long text at the end that goes on and on for a while.";

    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": long_text,
            "user_id": "center-test-user",
            "metadata": {"category": "center-test"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "special target query",
            "mode": "snippet"
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);
    let body = read_v1_json_body(search_res).await;
    let results = body["results"].as_array().expect("results should be array");
    assert!(!results.is_empty());

    let snippet = results[0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("special target query"));
    // Since the budget of snippet is 100 characters, it should center and not contain the start
    assert!(!snippet.contains("This is a very long text at the beginning"));
}

#[tokio::test]
async fn xtsp_clip_never_panics() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // Seed diverse complex multi-byte characters
    let complex_text = "🐙 emoji test 👨‍👩‍👧‍👦 family test 你好世界 boundary test";
    let add_res = post_v1_json(
        app.clone(),
        "/v1/memories",
        json!({
            "text": complex_text,
            "user_id": "panic-test-user",
            "metadata": {"category": "panic-test"}
        }),
    )
    .await;
    assert_eq!(add_res.status(), StatusCode::OK);

    // Search with snippet to trigger clip_chars under multi-byte conditions
    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "family",
            "mode": "snippet"
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);
    let body = read_v1_json_body(search_res).await;
    let results = body["results"].as_array().expect("results should be array");
    assert!(!results.is_empty());
}

#[tokio::test]
async fn xtsp_search_8kb_cap() {
    let _guard = TEST_MUTEX.lock().await;
    let (state, workspace, _server) = test_state().await;
    let app = v1_router(state, workspace);

    // Add several large memories (2KB of realistic text each) to trigger the 8KB truncation
    let base_paragraph = "Xavier cognitive memory architecture standard protocol spec is designed for high autonomous throughput. ".repeat(20);
    assert!(base_paragraph.len() > 2000);

    for i in 0..6 {
        let add_res = post_v1_json(
            app.clone(),
            "/v1/memories",
            json!({
                "text": format!("Document Block #{} - Metadata: {}", i, base_paragraph),
                "user_id": format!("user_8kb_{}", i),
                "metadata": {"title": format!("Large Cap #{}", i)}
            }),
        )
        .await;
        assert_eq!(add_res.status(), StatusCode::OK);
    }

    // Search in full mode (should return truncated: true and keep total response <= 8KB)
    let search_res = post_v1_json(
        app.clone(),
        "/v1/memories/search",
        json!({
            "query": "Document Block",
            "limit": 10
        }),
    )
    .await;
    assert_eq!(search_res.status(), StatusCode::OK);

    let response_bytes = axum::body::to_bytes(search_res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(response_bytes.len() <= 8192);

    let val: serde_json::Value = serde_json::from_slice(&response_bytes).expect("parse JSON");
    assert_eq!(val["status"], "ok");
    assert_eq!(val["truncated"], true);
}
