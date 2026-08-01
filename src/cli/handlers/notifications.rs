//! Notification handlers for the CLI server.

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
    Json,
};
use xavier::notifications::NOTIFICATIONS;

/// List notifications handler.
pub async fn list_notifications_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.list_notifications().await {
        Ok(notifications) => json_response(
            StatusCode::OK,
            serde_json::to_value(notifications).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

use serde::Deserialize;

#[derive(Deserialize)]
pub struct CreateSubscriptionPayload {
    pub url: String,
    pub event_types: Vec<String>,
}

/// Create subscription handler.
pub async fn create_subscription_handler(
    State(_state): State<CliState>,
    Json(payload): Json<CreateSubscriptionPayload>,
) -> Response {
    match NOTIFICATIONS
        .add_subscription(&payload.url, payload.event_types)
        .await
    {
        Ok(sub) => json_response(
            StatusCode::CREATED,
            serde_json::to_value(sub).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// List subscriptions handler.
pub async fn list_subscriptions_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.list_subscriptions().await {
        Ok(subs) => json_response(
            StatusCode::OK,
            serde_json::to_value(subs).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Delete subscription handler.
pub async fn delete_subscription_handler(
    State(_state): State<CliState>,
    Path(id): Path<String>,
) -> Response {
    match NOTIFICATIONS.remove_subscription(&id).await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Mark notification read handler.
pub async fn mark_notification_read_handler(
    State(_state): State<CliState>,
    Path(id): Path<String>,
) -> Response {
    match NOTIFICATIONS.mark_as_read(&id).await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Mark all notifications read handler.
pub async fn mark_all_notifications_read_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.mark_all_as_read().await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}

/// Delete all notifications handler.
pub async fn delete_all_notifications_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.delete_all().await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}
