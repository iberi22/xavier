//! Cloud Node settings handlers

use axum::{response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use crate::settings::XavierSettings;

#[derive(Debug, Serialize, Deserialize)]
pub struct CloudNodeConfig {
    pub url: Option<String>,
    pub token: Option<String>,
    pub instance_id: Option<String>,
}

pub async fn get_cloud_node() -> impl IntoResponse {
    let settings = XavierSettings::current();
    let config = CloudNodeConfig {
        url: settings.pgheart.url,
        token: settings.pgheart.token.as_ref().map(|_| "********".to_string()),
        instance_id: settings.pgheart.instance_id,
    };
    Json(serde_json::json!({ "status": "ok", "data": config }))
}

pub async fn update_cloud_node(
    Json(payload): Json<CloudNodeConfig>,
) -> impl IntoResponse {
    let mut settings = XavierSettings::current();

    if let Some(url) = payload.url {
        settings.pgheart.url = Some(url);
    }

    if let Some(token) = payload.token {
        if !token.contains("********") {
            settings.pgheart.token = Some(token);
        }
    }

    if let Some(instance_id) = payload.instance_id {
        settings.pgheart.instance_id = Some(instance_id);
    }

    match settings.save().await {
        Ok(_) => Json(serde_json::json!({ "status": "ok", "message": "Cloud node settings updated" })),
        Err(e) => Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
    }
}
