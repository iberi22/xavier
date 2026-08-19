//! Inbound HTTP handler for public node mesh directory
//!
//! Exposes public node discovery without revealing private nodes or credentials.

use crate::nodes::{NodeRegistry, PublicNodeInfo};
use axum::{http::StatusCode, response::IntoResponse, Json};

/// Handler for `GET /mesh/public/nodes` and `GET /v1/mesh/public/nodes`.
///
/// Returns only nodes registered with `NodeVisibility::Public`.
/// Private nodes are completely invisible.
pub async fn list_public_nodes_handler() -> impl IntoResponse {
    let registry = match NodeRegistry::open_default() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to open node registry: {}", e)
                })),
            )
                .into_response();
        }
    };

    let public_records = match registry.list_public() {
        Ok(records) => records,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to list public nodes: {}", e)
                })),
            )
                .into_response();
        }
    };

    let public_nodes: Vec<PublicNodeInfo> =
        public_records.iter().map(PublicNodeInfo::from).collect();

    (StatusCode::OK, Json(public_nodes)).into_response()
}
