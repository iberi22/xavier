//! MCP Session Edge & Lifecycle Test Suite
//!
//! Validates session states: abrupt client disconnects, progressive streaming timeouts,
//! payload size limit enforcement, and HTTP session management endpoints.

use axum::http::{HeaderMap, StatusCode};
use futures_util::stream;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use xavier::agents::RuntimeConfig;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::server::mcp::session::{
    execute_query_with_timeout, mcp_delete_handler, mcp_post_handler, mcp_sse_handler,
    stream_tool_response_with_cancellation, McpSessionError, McpSessionManager, McpSessionState,
};
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceRegistry, WorkspaceState};
use xavier::AppState;

fn test_unique_path(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}"))
}

async fn setup_test_app_state() -> (AppState, WorkspaceContext) {
    std::env::set_var("XAVIER_TOKEN", "test-token-mcp-edge");
    let db_path = test_unique_path("mcp-edge-code.db");
    let code_db =
        Arc::new(code_graph::db::CodeGraphDB::new(&db_path).expect("CodeGraphDB creation failed"));
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let workspace_registry = Arc::new(WorkspaceRegistry::new());
    let workspace = WorkspaceState::new(
        WorkspaceConfig {
            id: format!("mcp-edge-{}", ulid::Ulid::new()),
            token: "test-token-mcp-edge".to_string(),
            plan: xavier::workspace::PlanTier::Personal,
            memory_backend: xavier::memory::store::MemoryBackend::File,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(10_000),
            request_unit_limit: Some(20_000),
            embedding_provider_mode: xavier::workspace::EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: xavier::workspace::SyncPolicy::CloudMirror,
            dedup: xavier::settings::types::DedupSettings::default(),
        },
        RuntimeConfig::default(),
        test_unique_path("mcp-edge-store"),
    )
    .await
    .expect("WorkspaceState creation failed");
    workspace_registry
        .insert(workspace)
        .await
        .expect("insert workspace failed");
    let workspace_ctx = workspace_registry
        .authenticate("test-token-mcp-edge")
        .await
        .expect("authenticate failed");

    (
        AppState {
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
        },
        workspace_ctx,
    )
}

#[tokio::test]
async fn test_session_lifecycle_state_transitions() {
    let manager = McpSessionManager::new();
    let session_id = "test-session-lifecycle";

    let session = manager.create_session(session_id, None, None);
    assert_eq!(session.state, McpSessionState::Connected);
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::Connected)
    );

    // Transition Connected -> Streaming
    let prev = manager
        .transition_state(session_id, McpSessionState::Streaming)
        .expect("transition to streaming should succeed");
    assert_eq!(prev, McpSessionState::Connected);
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::Streaming)
    );

    // Transition Streaming -> Connected
    let prev = manager
        .transition_state(session_id, McpSessionState::Connected)
        .expect("transition to connected should succeed");
    assert_eq!(prev, McpSessionState::Streaming);
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::Connected)
    );

    // Disconnect handling
    manager
        .handle_disconnect(session_id)
        .expect("disconnect handling should succeed");
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::Disconnected)
    );

    // Graceful Close
    manager
        .close_session(session_id)
        .expect("close session should succeed");
    assert_eq!(manager.get_state(session_id), Some(McpSessionState::Closed));
}

#[tokio::test]
async fn test_payload_limit_enforcement() {
    let manager = McpSessionManager::new();
    let session_id = "test-session-payload-limit";

    // Set strict limit of 100 bytes
    manager.create_session(session_id, Some(100), None);

    // Transfer 60 bytes -> OK
    let transferred = manager
        .record_bytes(session_id, 60)
        .expect("recording 60 bytes should succeed");
    assert_eq!(transferred, 60);

    // Transfer another 50 bytes (total 110 > 100) -> fails with PayloadLimitExceeded
    let err = manager
        .record_bytes(session_id, 50)
        .expect_err("recording 50 bytes should exceed limit");

    match err {
        McpSessionError::PayloadLimitExceeded { limit, actual } => {
            assert_eq!(limit, 100);
            assert_eq!(actual, 110);
        }
        _ => panic!("Expected PayloadLimitExceeded, got {:?}", err),
    }

    // Verify session state is transitioned to Closed
    assert_eq!(manager.get_state(session_id), Some(McpSessionState::Closed));
}

#[tokio::test]
async fn test_query_timeout_handling() {
    let manager = McpSessionManager::new();
    let session_id = "test-session-timeout";
    manager.create_session(session_id, None, None);

    // Execute long-running query that times out
    let timeout_dur = Duration::from_millis(50);
    let result = execute_query_with_timeout(&manager, session_id, timeout_dur, async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        "completed"
    })
    .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        McpSessionError::TimedOut(dur) => assert_eq!(dur, timeout_dur),
        other => panic!("Expected TimedOut error, got {:?}", other),
    }

    // Verify session state updated to TimedOut
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::TimedOut)
    );

    // Execute fast query that completes within timeout
    let fast_result = execute_query_with_timeout(&manager, session_id, timeout_dur, async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        "fast_completed"
    })
    .await;

    assert_eq!(fast_result.unwrap(), "fast_completed");
    // Verify session state restored to Connected
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::Connected)
    );
}

#[tokio::test]
async fn test_abrupt_eof_during_large_tool_response() {
    let manager = McpSessionManager::new();
    let session_id = "test-session-abrupt-eof";
    manager.create_session(session_id, None, None);

    // Stream of 100 chunks
    let chunks: Vec<Vec<u8>> = (0..100).map(|i| vec![i as u8; 1024]).collect();
    let response_stream = stream::iter(chunks);

    // Channel with capacity 1
    let (tx, rx) = mpsc::channel(1);

    // Drop the receiver rx immediately to simulate abrupt EOF / client disconnect
    drop(rx);

    let result =
        stream_tool_response_with_cancellation(&manager, session_id, response_stream, tx).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        McpSessionError::Disconnected => {}
        other => panic!("Expected Disconnected error, got {:?}", other),
    }

    // Verify session state transitioned to Disconnected
    assert_eq!(
        manager.get_state(session_id),
        Some(McpSessionState::Disconnected)
    );
}

#[tokio::test]
async fn test_graceful_session_termination_delete_handler() {
    let session_id = "test-delete-handler-session";
    McpSessionManager::global().create_session(session_id, None, None);

    let mut headers = HeaderMap::new();
    headers.insert("mcp-session-id", session_id.parse().unwrap());

    let response = mcp_delete_handler(headers).await;
    assert_eq!(response.status(), StatusCode::OK);

    // Verify session state in global manager is Closed
    assert_eq!(
        McpSessionManager::global().get_state(session_id),
        Some(McpSessionState::Closed)
    );
}

#[tokio::test]
async fn test_mcp_sse_handler_connection() {
    let session_id = "test-sse-handler-session";
    let mut headers = HeaderMap::new();
    headers.insert("mcp-session-id", session_id.parse().unwrap());

    let response = mcp_sse_handler(headers).await;
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(
        McpSessionManager::global().get_state(session_id),
        Some(McpSessionState::Connected)
    );
}

#[tokio::test]
async fn test_stale_session_pruning() {
    let manager = McpSessionManager::new();
    let s1 = manager.create_session("s1", None, None);
    let _s2 = manager.create_session("s2", None, None);
    let _s3 = manager.create_session("s3", None, None);

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Refresh s1
    let _ = manager.transition_state(&s1.id, McpSessionState::Streaming);

    // Prune sessions older than 10ms idle time
    let pruned = manager.prune_stale_sessions(Duration::from_millis(10));
    assert_eq!(pruned, 2);
    assert_eq!(manager.active_session_count(), 1);
    assert!(manager.get_session("s1").is_some());
}

#[tokio::test]
async fn test_post_handler_payload_too_large() {
    let session_id = "test-post-handler-payload-too-large";
    McpSessionManager::global().create_session(session_id, Some(10), None);

    let (state, workspace) = setup_test_app_state().await;
    let mut headers = HeaderMap::new();
    headers.insert("mcp-session-id", session_id.parse().unwrap());

    // Body larger than 10 bytes limit
    let body = axum::body::Bytes::from(
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        })
        .to_string(),
    );

    let response = mcp_post_handler(
        axum::extract::State(state),
        axum::extract::Extension(workspace),
        None,
        headers,
        body,
    )
    .await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn test_mcp_session_error_display_formatting() {
    let err_not_found = McpSessionError::SessionNotFound("s99".to_string());
    assert!(err_not_found.to_string().contains("Session not found: s99"));

    let err_limit = McpSessionError::PayloadLimitExceeded {
        limit: 100,
        actual: 200,
    };
    assert!(err_limit
        .to_string()
        .contains("200 bytes exceeds limit of 100 bytes"));

    let err_timeout = McpSessionError::TimedOut(Duration::from_secs(5));
    assert!(err_timeout.to_string().contains("Session timed out"));

    let err_disconnect = McpSessionError::Disconnected;
    assert!(err_disconnect.to_string().contains("Session disconnected"));

    let err_transition = McpSessionError::InvalidStateTransition {
        from: McpSessionState::Closed,
        to: McpSessionState::Streaming,
    };
    assert!(err_transition
        .to_string()
        .contains("Invalid session state transition"));
}
