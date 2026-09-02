//! HTTP handler module re-exports
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
pub mod agent;
pub mod ivn;
pub mod marketplace;
pub mod memory;
pub mod nodes;
pub mod security;
pub mod sync;
pub mod training;

pub use agent::*;
pub use ivn::*;
pub use marketplace::*;
pub use memory::*;
pub use nodes::*;
pub use security::*;
pub use sync::*;
pub use training::*;

use axum::{http::StatusCode, Json};

/// Helper function to create a standardized error JSON response body.
pub fn error_json(message: impl std::fmt::Display) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "error",
        "message": message.to_string(),
    }))
}

/// Helper function to create a standardized error response with status code and JSON body.
pub fn error_response(
    status: StatusCode,
    message: impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    (status, error_json(message))
}
