//! Node provisioning HTTP handlers (Olas M6/M7, REQ-029/030)
//!
//! Provides public discovery endpoints for SWAL nodes:
//! - `GET /mesh/public/nodes`: Lists public nodes (PublicNodeInfo).
//!   Private nodes are completely invisible (not listed, no hints).
//!   Secrets, leases, and private keys are never exposed.

use axum::{http::StatusCode, response::Response};

use crate::cli::handlers::json_response;
use xavier::nodes::{NodeRegistry, PublicNodeInfo};

/// Handler for `GET /mesh/public/nodes` and `GET /v1/mesh/public/nodes`.
///
/// Returns only nodes registered with `NodeVisibility::Public`.
/// Responses contain only sanitized `PublicNodeInfo` metadata.
pub async fn list_public_nodes_handler() -> Response {
    let registry = match NodeRegistry::open_default() {
        Ok(r) => r,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": format!("Failed to open node registry: {}", e)
                }),
            );
        }
    };

    let public_records = match registry.list_public() {
        Ok(records) => records,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "error": format!("Failed to list public nodes: {}", e)
                }),
            );
        }
    };

    let public_nodes: Vec<PublicNodeInfo> =
        public_records.iter().map(PublicNodeInfo::from).collect();

    json_response(
        StatusCode::OK,
        serde_json::to_value(public_nodes).unwrap_or_default(),
    )
}

/// Cloudflare Tunnel auto-register shard helper (WAVE-2.04)
pub fn shard_for_tunnel(id: &str) -> u8 {
    xavier_lib::crypto::envelope::shard_for_id(id)
}
pub fn tunnel_url_for_id(id: &str) -> String {
    format!(
        "https://xavier-{}.trycloudflare.com",
        &id[..8.min(id.len())]
    )
}
pub fn cloudflared_cmd(id: &str) -> String {
    format!(
        "cloudflared tunnel --url http://localhost:8006 --hostname {}",
        tunnel_url_for_id(id)
    )
}
