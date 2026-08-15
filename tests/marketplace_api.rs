use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use xavier::adapters::inbound::http::routes::create_router;

#[tokio::test]
async fn test_marketplace_full_lifecycle() {
    let router = create_router();

    // 1. GET /v1/marketplace/pricing preview
    let pricing_req = Request::builder()
        .uri("/v1/marketplace/pricing?size=500&reputation=0.5")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(pricing_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let pricing_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(pricing_json["status"], "ok");
    assert_eq!(pricing_json["preview_size"], 500);
    assert_eq!(pricing_json["pricing_tiers"]["Free"], 0);
    assert!(pricing_json["pricing_tiers"]["Colaborador"].as_u64().unwrap() > 0);

    // 2. POST /v1/marketplace/datasets list dataset
    let list_payload = json!({
        "name": "Integration Test Telemetry",
        "description": "Realtime logs from node cluster",
        "category": "Telemetry",
        "publisher": "xv1_publisher_wallet_test_1234567890",
        "tier": "Colaborador",
        "reputation": 0.0,
        "rows": [
            { "node_id": "xv1-node-alpha", "status": "active", "load": 22 },
            { "node_id": "xv1-node-beta", "status": "overloaded", "load": 99 },
            { "node_id": "xv1-node-gamma", "status": "idle", "load": 5 }
        ]
    });

    let list_req = Request::builder()
        .uri("/v1/marketplace/datasets")
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&list_payload).unwrap()))
        .unwrap();

    let response = router.clone().oneshot(list_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let list_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(list_json["status"], "ok");
    let dataset_id = list_json["dataset_id"].as_str().unwrap().to_string();
    assert!(dataset_id.starts_with("ds_"));

    // 3. GET /v1/marketplace/datasets list active datasets
    let get_req = Request::builder()
        .uri("/v1/marketplace/datasets")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(get_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let active_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(active_json["status"], "ok");
    assert!(active_json["count"].as_u64().unwrap() >= 1);

    // Verify rows are stripped in public metadata
    let datasets = active_json["datasets"].as_array().unwrap();
    let listed = datasets.iter().find(|d| d["id"] == dataset_id).unwrap();
    assert_eq!(listed["rows"], json!([]));

    // 4. POST /v1/marketplace/datasets/{id}/query
    let query_payload = json!({
        "query": "overloaded",
        "payment": 100
    });

    let query_req = Request::builder()
        .uri(format!("/v1/marketplace/datasets/{}/query", dataset_id))
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&query_payload).unwrap()))
        .unwrap();

    let response = router.clone().oneshot(query_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let query_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(query_json["status"], "ok");
    let records = query_json["page"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["node_id"], "xv1-node-beta");

    // 5. DELETE /v1/marketplace/datasets/{id} revoke dataset
    let revoke_req = Request::builder()
        .uri(format!("/v1/marketplace/datasets/{}", dataset_id))
        .method(Method::DELETE)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(revoke_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let revoke_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(revoke_json["status"], "ok");

    // Verify query fails after revocation
    let query_post_revoke = Request::builder()
        .uri(format!("/v1/marketplace/datasets/{}/query", dataset_id))
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&query_payload).unwrap()))
        .unwrap();

    let response = router.clone().oneshot(query_post_revoke).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let err_json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err_json["status"], "error");
    assert!(err_json["message"].as_str().unwrap().contains("revoked"));
}
