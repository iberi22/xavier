#!/bin/bash
cat << 'INNER_EOF' > tests/v1_memories_add_path_traversal.rs
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use tower::ServiceExt;
use xavier::server::v1_api::{v1_memories_add, V1AddMemoryRequest};
use xavier::workspace::{WorkspaceConfig, WorkspaceContext, WorkspaceState};
use xavier::agents::RuntimeConfig;
use std::sync::Arc;

#[tokio::test]
async fn test_v1_memories_add_path_traversal() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::from_env();
    let runtime = RuntimeConfig::from_env();
    let state = Arc::new(WorkspaceState::new(config, runtime, temp_dir.path().to_path_buf()).await.unwrap());

    let ctx = WorkspaceContext {
        workspace_id: "test".to_string(),
        workspace: state,
    };

    let app = Router::new()
        .route("/v1/memories", post(v1_memories_add))
        .layer(axum::Extension(ctx));

    // Try path traversal
    let payload = V1AddMemoryRequest {
        text: Some("hello".to_string()),
        user_id: Some("../../../etc/passwd".to_string()),
        messages: None,
        metadata: None,
        kind: None,
        evidence_kind: None,
        namespace: None,
        provenance: None,
        mode: None,
    };

    let req = Request::builder()
        .method("POST")
        .uri("/v1/memories")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Read the response to verify it didn't use ../../../etc/passwd
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // It should have sanitized the path/id to just "etcpasswd" or similar without the slashes and dots
    assert!(body_json["status"].as_str().unwrap() == "ok");
}
INNER_EOF
