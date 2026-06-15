//! CLI Request Handlers
//!
//! This module re-exports handlers from sub-modules for cleaner organization.

pub mod agent;
pub mod code;
pub mod headless_api;
pub mod headless_e2e;
pub mod memory;
pub mod mesh;
pub mod navigation;
pub mod notifications;
pub mod onboarding;
pub mod panel;
pub mod quota;
pub mod secrets;
pub mod security;
pub mod setup;
pub mod system;
pub mod system_scan;
pub mod tasks;
pub mod tokens;
pub mod usage;
pub mod workspace;

pub use agent::*;
pub use code::*;
pub use memory::*;
pub use mesh::*;
pub use onboarding::*;
pub use panel::*;
pub use secrets::*;
pub use security::*;
pub use system::*;
pub use tasks::*;
pub use tokens::*;

pub use usage::*;
pub use workspace::*;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Common helper for JSON responses.
pub fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("x-request-id", uuid::Uuid::new_v4().to_string())
        .body(axum::body::Body::from(body.to_string()))
        .unwrap_or_else(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"status":"error"}).to_string(),
            )
                .into_response()
        })
}
