//! feat-cli-health-stats-parity: Verify that /memory/stats and /v1/stats
//! route to the live stats implementation correctly and are not unmounted or stubs.

use axum::http::StatusCode;
use serde_json::Value;
use serial_test::serial;

xavier::isolate_test_process!();

#[path = "router_harness/mod.rs"]
mod router_harness;
use router_harness::{jwt, test_app, RestoreEnv};

#[tokio::test]
#[serial]
async fn memory_stats_routes_match_cli_mcp() {
    // Hermetic env: temp config/runtime state and no real embedding provider
    // (memory POSTs spawn an embedding backfill). Guards restore on drop.
    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let _env_paths = RestoreEnv::set(&[
        ("XAVIER_CONFIG_PATH", data_dir.join("config.toml").as_path()),
        ("XAVIER_RUNTIME_STATE_PATH", data_dir.as_path()),
    ]);
    let _env_embed = RestoreEnv::set_str(&[
        ("XAVIER_EMBEDDER", "disabled"),
        ("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled"),
    ]);
    let _env_clear = RestoreEnv::clear(&[
        "XAVIER_EMBEDDING_URL",
        "XAVIER_EMBEDDING_LOCAL_URL",
        "OPENAI_API_KEY",
    ]);

    let t = test_app().await;

    // Add two memories
    let auth = jwt(xavier::security::auth::UserRole::Admin);

    for i in 0..2 {
        let payload = serde_json::json!({
            "path": format!("activity/stats/events/{}", i),
            "content": format!("Content for stats {}", i),
            "metadata": {"index": i, "clearance": "unclassified"}
        });

        let req = axum::http::Request::builder()
            .uri("/v1/memories")
            .method("POST")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", auth))
            .body(axum::body::Body::from(payload.to_string()))
            .unwrap();

        let res = tower::ServiceExt::oneshot(t.app.clone(), req)
            .await
            .unwrap();
        let status = res.status();
        let body_bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&body_bytes);
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // GET /memory/stats
    let req1 = axum::http::Request::builder()
        .uri("/memory/stats")
        .method("GET")
        .header("authorization", format!("Bearer {}", auth))
        .body(axum::body::Body::empty())
        .unwrap();
    let res1 = tower::ServiceExt::oneshot(t.app.clone(), req1)
        .await
        .unwrap();
    assert_eq!(res1.status(), StatusCode::OK);
    let body1 = axum::body::to_bytes(res1.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json1: Value = serde_json::from_slice(&body1).unwrap();

    // GET /v1/stats
    let req2 = axum::http::Request::builder()
        .uri("/v1/stats")
        .method("GET")
        .header("authorization", format!("Bearer {}", auth))
        .body(axum::body::Body::empty())
        .unwrap();
    let res2 = tower::ServiceExt::oneshot(t.app.clone(), req2)
        .await
        .unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let body2 = axum::body::to_bytes(res2.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json2: Value = serde_json::from_slice(&body2).unwrap();

    // Check count and shape
    for (route, json) in [("/memory/stats", &json1), ("/v1/stats", &json2)] {
        let total = json["total_memories"]
            .as_u64()
            .unwrap_or_else(|| panic!("{route}: expected total_memories field, got {json}"));
        assert!(
            total >= 2,
            "{route}: expected at least 2 memories, got {total}"
        );
    }

    assert_eq!(
        json1, json2,
        "Both stats routes must return the identical shape"
    );

    // Check that it's not the old stub
    assert_eq!(
        json1.get("message"),
        None,
        "Should not contain stub message"
    );
    let json_str = serde_json::to_string(&json1).unwrap();
    assert!(!json_str.contains("not yet implemented"));
}
