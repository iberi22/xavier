//! <feat-headless-server-packaging>: Headless tools and agent routes contract verification.

xavier::isolate_test_process!();

mod router_harness;

use axum::http::StatusCode;
use router_harness::{test_app, RestoreEnv, ROOT_TOKEN};
use serde_json::Value;

#[tokio::test]
#[serial_test::serial]
async fn headless_unsupported_tool_returns_contract() {
    let _guard = crate::SUITE_ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let _env_dirs = RestoreEnv::set(&[
        ("HOME", temp.path()),
        ("XAVIER_DATA_DIR", temp.path()),
        ("XAVIER_CONFIG_PATH", temp.path()),
        ("XAVIER_RUNTIME_STATE_PATH", temp.path()),
    ]);
    let _env_flags = RestoreEnv::set_str(&[("XAVIER_HEADLESS", "true")]);

    let t = test_app().await;

    // Unknown tool
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/headless/tools/unknown_tool")
        .header("Authorization", format!("Bearer {ROOT_TOKEN}"))
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{"args": {}}"#))
        .unwrap();

    let resp = tower::ServiceExt::oneshot(t.app.clone(), req)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    let body_bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body["code"], 501);
    assert_eq!(body["reason"], "unsupported_in_headless");
    assert!(body["supported_tools"].is_array());
    assert!(body["supported_tools"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("code_scan")));
}

#[tokio::test]
#[serial_test::serial]
async fn headless_supported_tool_does_not_return_501() {
    let _guard = crate::SUITE_ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let _env_dirs = RestoreEnv::set(&[
        ("HOME", temp.path()),
        ("XAVIER_DATA_DIR", temp.path()),
        ("XAVIER_CONFIG_PATH", temp.path()),
        ("XAVIER_RUNTIME_STATE_PATH", temp.path()),
    ]);
    let _env_flags = RestoreEnv::set_str(&[("XAVIER_HEADLESS", "true")]);

    let t = test_app().await;

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/headless/tools/code_scan")
        .header("Authorization", format!("Bearer {ROOT_TOKEN}"))
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{"args": {}}"#))
        .unwrap();

    let resp = tower::ServiceExt::oneshot(t.app.clone(), req)
        .await
        .unwrap();
    // It might return OK, 400, or 500 depending on DB state, but NOT 501 Not Implemented.
    assert_ne!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
#[serial_test::serial]
async fn headless_agent_endpoints_return_contract() {
    let _guard = crate::SUITE_ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().expect("tempdir");
    let _env_dirs = RestoreEnv::set(&[
        ("HOME", temp.path()),
        ("XAVIER_DATA_DIR", temp.path()),
        ("XAVIER_CONFIG_PATH", temp.path()),
        ("XAVIER_RUNTIME_STATE_PATH", temp.path()),
    ]);
    let _env_flags = RestoreEnv::set_str(&[("XAVIER_HEADLESS", "true")]);

    let t = test_app().await;

    // GET /v1/agents
    let req1 = axum::http::Request::builder()
        .method("GET")
        .uri("/v1/agents")
        .header("Authorization", format!("Bearer {ROOT_TOKEN}"))
        .body(axum::body::Body::empty())
        .unwrap();

    let resp1 = tower::ServiceExt::oneshot(t.app.clone(), req1)
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::NOT_IMPLEMENTED);
    let body_bytes1 = axum::body::to_bytes(resp1.into_body(), 65536)
        .await
        .unwrap();
    let body1: Value = serde_json::from_slice(&body_bytes1).unwrap();
    assert_eq!(body1["code"], 501);
    assert_eq!(body1["reason"], "unsupported_in_headless");
    assert_eq!(body1["hint"], "run the full daemon (xavier serve)");

    // POST /v1/agents/spawn
    let req2 = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/agents/spawn")
        .header("Authorization", format!("Bearer {ROOT_TOKEN}"))
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(r#"{"count":1,"skills":[]}"#))
        .unwrap();

    let resp2 = tower::ServiceExt::oneshot(t.app.clone(), req2)
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::NOT_IMPLEMENTED);
    let body_bytes2 = axum::body::to_bytes(resp2.into_body(), 65536)
        .await
        .unwrap();
    let body2: Value = serde_json::from_slice(&body_bytes2).unwrap();
    assert_eq!(body2["code"], 501);
    assert_eq!(body2["reason"], "unsupported_in_headless");
    assert_eq!(body2["hint"], "run the full daemon (xavier serve)");
}
