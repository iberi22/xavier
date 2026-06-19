use axum::{
    extract::{Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use xavier::enterprise::rbac::Role;
use xavier::memory::schema::ClearanceLevel;
use xavier::mesh::{
    NodeId, NodeIdentity, PeerInfo, PeerRegistry,
    acl::{MeshAcl, NodeAclEntry},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerResponse {
    pub node_id: String,
    pub alias: Option<String>,
    pub endpoint_url: String,
    pub role: Role,
    pub clearance: ClearanceLevel,
    pub last_seen_at: Option<i64>,
    pub sync_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerListResponse {
    pub peers: Vec<PeerResponse>,
    pub local_node_id: String,
}

#[derive(Debug, Deserialize)]
pub struct PairRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAclRequest {
    pub role: Role,
    pub clearance: ClearanceLevel,
}

#[derive(Debug, Serialize)]
pub struct PairingCodeResponse {
    pub code: String,
    pub secret: String,
}

pub async fn list_peers_handler() -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let registry = match PeerRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let acl = match MeshAcl::load() {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let identity = match NodeIdentity::load_or_create() {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let peers = registry
        .list_peers()
        .into_iter()
        .map(|p| {
            let entry = acl.get_entry(&p.node_id);
            PeerResponse {
                node_id: p.node_id.0.clone(),
                alias: p.alias.clone(),
                endpoint_url: p.endpoint_url.clone(),
                role: entry.as_ref().map(|e| e.role.clone()).unwrap_or(Role::Viewer),
                clearance: entry.as_ref().map(|e| e.clearance.clone()).unwrap_or(ClearanceLevel::Unclassified),
                last_seen_at: p.last_seen_at,
                sync_enabled: p.sync_enabled,
            }
        })
        .collect();

    Json(PeerListResponse {
        peers,
        local_node_id: identity.node_id.0,
    })
    .into_response()
}

pub async fn generate_pairing_code_handler(
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let endpoint = payload
        .get("endpoint")
        .and_then(|v| v.as_str())
        .unwrap_or("http://localhost:8006");
    let identity = match NodeIdentity::load_or_create() {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let (code, secret) = xavier::mesh::pairing::generate_pairing_code(
        identity.node_id.clone(),
        endpoint.to_string(),
        xavier::crypto::hex_encode(&identity.public_key),
    );

    Json(PairingCodeResponse { code, secret }).into_response()
}

#[derive(Debug, Serialize)]
pub struct DecodedPairingCodeResponse {
    pub node_id: String,
    pub endpoint: String,
}

pub async fn decode_pairing_code_handler(Json(payload): Json<PairRequest>) -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match xavier::mesh::pairing::decode_pairing_code(&payload.code) {
        Ok(data) => Json(DecodedPairingCodeResponse {
            node_id: data.node_id.0,
            endpoint: data.endpoint,
        })
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn pair_peer_handler(Json(payload): Json<PairRequest>) -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let data = match xavier::mesh::pairing::decode_pairing_code(&payload.code) {
        Ok(d) => d,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let mut registry = match PeerRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let peer = PeerInfo {
        node_id: data.node_id.clone(),
        alias: None,
        endpoint_url: data.endpoint.clone(),
        public_key_hex: data.public_key_hex.clone(),
        added_at: chrono::Utc::now().timestamp(),
        last_seen_at: None,
        sync_enabled: true,
        is_cloud: false,
    };

    if let Err(e) = registry.add_peer(peer) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    // Default ACL for new paired peer
    let mut acl = match MeshAcl::load() {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if let Err(e) = acl.set_entry(
        data.node_id.clone(),
        NodeAclEntry {
            role: Role::Viewer,
            clearance: ClearanceLevel::Unclassified,
            namespaces: None,
            public_key_hex: data.public_key_hex.clone(),
        },
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    info!("Paired with node {}", data.node_id);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ok", "node_id": data.node_id.0 })),
    )
        .into_response()
}

pub async fn update_peer_acl_handler(
    Path(node_id): Path<String>,
    Json(payload): Json<UpdateAclRequest>,
) -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let node_id = NodeId(node_id);
    let mut acl = match MeshAcl::load() {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let existing = acl.get_entry(&node_id).cloned();

    if let Err(e) = acl.set_entry(
        node_id.clone(),
        NodeAclEntry {
            role: payload.role,
            clearance: payload.clearance,
            namespaces: existing.as_ref().and_then(|entry| entry.namespaces.clone()),
            public_key_hex: existing
                .map(|entry| entry.public_key_hex)
                .unwrap_or_default(),
        },
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
}

pub async fn remove_peer_handler(Path(node_id): Path<String>) -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let node_id = NodeId(node_id);
    let mut registry = match PeerRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if let Err(e) = registry.remove_peer(&node_id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    let mut acl = match MeshAcl::load() {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if let Err(e) = acl.remove_entry(&node_id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
}
