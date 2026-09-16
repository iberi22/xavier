use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use serde_json::Value;
use tower::ServiceExt;
use xavier::cli::handlers::config::get_messaging_status_handler;

#[tokio::test]
async fn test_messaging_status_endpoint() {
    let app = Router::new().route("/v1/messaging/status", get(get_messaging_status_handler));

    let request = Request::builder()
        .uri("/v1/messaging/status")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert!(json.get("discord").is_some(), "discord field missing");
    assert!(json.get("slack").is_some(), "slack field missing");
    assert!(json.get("telegram").is_some(), "telegram field missing");
    assert!(json.get("whatsapp").is_some(), "whatsapp field missing");
    assert!(json.get("teams").is_some(), "teams field missing");

    let telegram = json.get("telegram").unwrap();
    assert!(telegram.get("configured").is_some());
    assert!(telegram.get("active").is_some());

    let discord = json.get("discord").unwrap();
    assert!(discord.get("configured").is_some());
    assert!(discord.get("active").is_some());
}
