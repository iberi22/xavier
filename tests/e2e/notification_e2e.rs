use axum::{
    body::Body,
    extract::Path,
    http::{Method, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::util::ServiceExt;

use xavier::{
    codebase::connection_manager::ConnectionManager,
    notifications::{IslandId, NOTIFICATIONS},
    storage::migrations,
};

fn unique_test_path(prefix: &str, suffix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime before UNIX_EPOCH")
        .as_nanos();
    let tid = std::thread::current().id();
    std::env::temp_dir().join(format!("{prefix}-{unique:016x}-{tid:?}-{suffix}"))
}

// Simple endpoints matching the contract of actual handlers
async fn list_notifications_handler() -> Response {
    match NOTIFICATIONS.list_notifications().await {
        Ok(notifications) => (StatusCode::OK, Json(notifications)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn mark_notification_read_handler(Path(id): Path<String>) -> Response {
    match NOTIFICATIONS.mark_as_read(&id).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn mark_all_notifications_read_handler() -> Response {
    match NOTIFICATIONS.mark_all_as_read().await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn delete_all_notifications_handler() -> Response {
    match NOTIFICATIONS.delete_all().await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok" }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn build_router() -> Router {
    Router::new()
        .route("/notifications", get(list_notifications_handler))
        .route(
            "/notifications/{id}/read",
            post(mark_notification_read_handler),
        )
        .route(
            "/notifications/read-all",
            post(mark_all_notifications_read_handler),
        )
        .route(
            "/notifications/all",
            delete(delete_all_notifications_handler),
        )
}

async fn get_json_body(response: Response) -> Value {
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body failed");
    serde_json::from_slice(&body_bytes).expect("parse JSON response body failed")
}

#[tokio::test]
async fn test_notification_system_e2e() {
    // 1. Setup isolated database for notifications
    let db_path = unique_test_path("xavier-e2e-notifications", "xavier_memory.db");
    ConnectionManager::global()
        .connect_with_path("memory", db_path.clone())
        .expect("Failed to connect to isolated memory database");

    // Run migrations to ensure `notifications` table exists
    ConnectionManager::global()
        .with_conn("memory", |conn| {
            migrations::run(conn).expect("Failed to run migrations");
            Ok(())
        })
        .await
        .expect("Failed to run database migrations for notification test");

    let app = build_router();

    // 2. Dispatch notifications
    let note1 = NOTIFICATIONS
        .notify(
            IslandId::System,
            "System Startup",
            "Xavier Cognitive Runtime started successfully.",
            "success",
        )
        .await
        .expect("Failed to dispatch first notification");

    let note2 = NOTIFICATIONS
        .notify(
            IslandId::Memory,
            "Memory Consolidated",
            "Background consolidation successfully completed.",
            "info",
        )
        .await
        .expect("Failed to dispatch second notification");

    // 3. List notifications via HTTP Router
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/notifications")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to call list notifications endpoint");

    assert_eq!(response.status(), StatusCode::OK);
    let list: Vec<Value> = serde_json::from_value(get_json_body(response).await).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0]["title"], "Memory Consolidated");
    assert_eq!(list[0]["read"], false);
    assert_eq!(list[1]["title"], "System Startup");

    // 4. Mark specific notification as read
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/notifications/{}/read", note1.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to call read notification endpoint");

    assert_eq!(response.status(), StatusCode::OK);

    // Verify it is marked as read
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/notifications")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list: Vec<Value> = serde_json::from_value(get_json_body(response).await).unwrap();
    let note1_updated = list.iter().find(|n| n["id"] == note1.id).unwrap();
    assert_eq!(note1_updated["read"], true);
    let note2_updated = list.iter().find(|n| n["id"] == note2.id).unwrap();
    assert_eq!(note2_updated["read"], false);

    // 5. Mark all as read
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/notifications/read-all")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify all marked as read
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/notifications")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list: Vec<Value> = serde_json::from_value(get_json_body(response).await).unwrap();
    for note in list {
        assert_eq!(note["read"], true);
    }

    // 6. Delete all notifications
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/notifications/all")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify notifications deleted
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/notifications")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list: Vec<Value> = serde_json::from_value(get_json_body(response).await).unwrap();
    assert!(list.is_empty());

    // Clean up
    if db_path.exists() {
        std::fs::remove_file(db_path).ok();
    }
}
