//! Notification handlers for the CLI server.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
    Json,
};
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::settings::XavierSettings;
use xavier::notifications::{NOTIFICATIONS};

pub async fn list_notifications_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.list_notifications().await {
        Ok(notifications) => json_response(StatusCode::OK, serde_json::to_value(notifications).unwrap_or_default()),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn get_notification_settings_handler(State(_state): State<CliState>) -> Response {
    let settings = XavierSettings::current();
    json_response(StatusCode::OK, serde_json::to_value(&settings.server.notifications).unwrap_or_default())
}

#[derive(serde::Deserialize)]
pub struct UpdateNotificationSettingsRequest {
    pub enabled_islands: Option<Vec<String>>,
    pub sound_enabled: Option<bool>,
}

pub async fn update_notification_settings_handler(
    State(_state): State<CliState>,
    Json(payload): Json<UpdateNotificationSettingsRequest>,
) -> Response {
    let mut settings = XavierSettings::current();
    if let Some(islands) = payload.enabled_islands {
        settings.server.notifications.enabled_islands = islands;
    }
    if let Some(sound) = payload.sound_enabled {
        settings.server.notifications.sound_enabled = sound;
    }

    match settings.save().await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn mark_notification_read_handler(
    State(_state): State<CliState>,
    Path(id): Path<String>,
) -> Response {
    match NOTIFICATIONS.mark_as_read(&id).await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn mark_all_notifications_read_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.mark_all_as_read().await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn delete_all_notifications_handler(State(_state): State<CliState>) -> Response {
    match NOTIFICATIONS.delete_all().await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": e.to_string() })),
    }
}
