//! CLI Request Handlers
//!
//! This module re-exports handlers from sub-modules for cleaner organization.

pub mod agent;
pub mod agent_cli;
pub mod auth;
pub mod billing;
pub mod cloud;
pub mod code;
pub mod doctor;
pub mod headless_api;
pub mod headless_e2e;
pub mod memory;
pub mod mesh;
pub mod navigation;
pub mod notifications;
pub mod offline_models;
pub mod ollama_models;
pub mod onboarding;
pub mod panel;
pub mod quota;
pub mod recovery;
pub mod secrets;
pub mod security;
pub mod setup;
pub mod sync;
pub mod system;
pub mod system_scan;
pub mod system_scan_cli;
pub mod tasks;
pub mod tokens;
pub mod usage;
pub mod verify;
pub mod workspace;
pub mod workspace_db;

pub use agent::*;
pub use workspace_db::*;
pub use agent_cli::*;
pub use auth::*;
pub use code::*;
pub use memory::*;
pub use mesh::*;
pub use onboarding::*;
pub use panel::*;
pub use recovery::*;
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
#[cfg(test)]
pub mod proxy_auth_tests;
