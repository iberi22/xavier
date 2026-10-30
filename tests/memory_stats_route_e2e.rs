//! feat-cli-health-stats-parity: Verify that /memory/stats and /v1/stats
//! route to the live stats implementation correctly and are not unmounted or stubs.

use axum::http::{Method, StatusCode};
use serde_json::Value;

xavier::isolate_test_process!();

mod router_harness;
use router_harness::{jwt, status, test_app, ROOT_TOKEN};

#[tokio::test]
async fn memory_stats_routes_match_cli_mcp() {
    let t = test_app().await;

    // Add two memories
    let auth = jwt(xavier::security::auth::UserRole::Admin);

    for i in 0..2 {
        let payload = serde_json::json!({
            "path": format!("activity/stats/events/{}", i),
            "text": format!("Content for stats {}", i),
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
        assert_eq!(res.status(), StatusCode::OK);
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
    let count1 = json1["memory_count"]
        .as_u64()
        .expect("Expected memory_count field");
    assert!(count1 >= 2, "Expected at least 2 memories, got {}", count1);

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
