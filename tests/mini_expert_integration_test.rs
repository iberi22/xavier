use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use xavier::adapters::inbound::http::routes::create_router;

#[tokio::test]
async fn test_mini_experts_list_integration() {
    let response = create_router()
        .oneshot(
            Request::builder()
                .uri("/v1/agents/mini-experts")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(parsed.is_array());
    let list = parsed.as_array().unwrap();
    assert!(!list.is_empty());

    // Look for our default experts
    let names: Vec<&str> = list.iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"agy-expert"));
    assert!(names.contains(&"local-expert"));
    assert!(names.contains(&"custom-expert"));
}

#[tokio::test]
async fn test_mini_expert_invoke_integration() {
    let req_body = serde_json::json!({
        "name": "custom-expert",
        "prompt": "Hello integration test"
    });

    let response = create_router()
        .oneshot(
            Request::builder()
                .uri("/v1/agents/mini-experts/invoke")
                .method(Method::POST)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["status"], "success");
    assert_eq!(parsed["provider"], "custom");
    assert!(parsed["response"]
        .as_str()
        .unwrap()
        .contains("Mock response"));
}

#[tokio::test]
async fn test_mini_expert_invoke_not_found_integration() {
    let req_body = serde_json::json!({
        "name": "non-existent-expert",
        "prompt": "Hello integration test"
    });

    let response = create_router()
        .oneshot(
            Request::builder()
                .uri("/v1/agents/mini-experts/invoke")
                .method(Method::POST)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
