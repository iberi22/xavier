use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use crate::cli::state::CliState;
use crate::cli::server::json_response;
use serde::{Deserialize, Serialize};
use tracing::info;
use xavier::enterprise::rbac::Role;
use xavier::memory::schema::{ClearanceLevel, FederatedSearchRequest};
use xavier::mesh::{
    acl::{MeshAcl, NodeAclEntry},
    MeshMaturityReport, NodeId, NodeIdentity, PeerInfo, PeerRegistry,
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
                role: entry
                    .as_ref()
                    .map(|e| e.role.clone())
                    .unwrap_or(Role::Viewer),
                clearance: entry
                    .as_ref()
                    .map(|e| e.clearance.clone())
                    .unwrap_or(ClearanceLevel::Unclassified),
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
        iroh_addr: None,
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: std::collections::HashMap::new(),
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
            namespace_acl: None,
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
                .as_ref()
                .map(|entry| entry.public_key_hex.clone())
                .unwrap_or_default(),
            namespace_acl: existing.as_ref().and_then(|entry| entry.namespace_acl.clone()),
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

#[derive(Debug, Deserialize)]
pub struct RevokeConsentRequest {
    pub token_id: String,
}

pub async fn revoke_consent_handler(
    Json(payload): Json<RevokeConsentRequest>,
) -> impl IntoResponse {
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match xavier::mesh::DataConsentManager::revoke_consent(&payload.token_id) {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "ok", "message": "Consent revoked successfully" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to revoke consent: {}", e) })),
        )
            .into_response(),
    }
}

pub async fn list_consents_handler() -> impl IntoResponse {
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    match xavier::mesh::DataConsentManager::list_active_consents() {
        Ok(consents) => (
            StatusCode::OK,
            Json(serde_json::json!({ "active_consents": consents })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to list active consents: {}", e) })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateBridgeRequest {
    pub source_db: String,
    pub source_namespace: String,
    pub target_db: String,
    pub bridge_kind: xavier::mesh::BridgeKind,
    pub acl: Vec<String>,
}

pub async fn create_bridge_handler(
    Json(payload): Json<CreateBridgeRequest>,
) -> impl IntoResponse {
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let mut registry = match xavier::mesh::BridgeRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let id = uuid::Uuid::new_v4().to_string();
    let bridge = xavier::mesh::ContextBridge {
        id,
        source_db: payload.source_db,
        source_namespace: payload.source_namespace,
        target_db: payload.target_db,
        bridge_kind: payload.bridge_kind,
        acl: payload.acl,
    };

    if let Err(e) = registry.add_bridge(bridge.clone()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    (StatusCode::CREATED, Json(bridge)).into_response()
}

pub async fn list_bridges_handler() -> impl IntoResponse {
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let registry = match xavier::mesh::BridgeRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    Json(registry.list_bridges()).into_response()
}

pub async fn delete_bridge_handler(
    Path(id): Path<String>,
) -> impl IntoResponse {
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    let mut registry = match xavier::mesh::BridgeRegistry::load() {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    if let Err(e) = registry.remove_bridge(&id) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }

    Json(serde_json::json!({ "status": "ok" })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ShareWorkspaceRequest {
    pub workspace_id: String,
    pub namespaces: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct ShareWorkspaceResponse {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct JoinWorkspaceRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct JoinWorkspaceResponse {
    pub status: String,
    pub node_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct QueryWorkspaceRequest {
    pub token: String,
    pub query: String,
    pub limit: Option<usize>,
    #[serde(default)]
    pub federated: Option<FederatedSearchRequest>,
}

pub async fn share_workspace_handler(
    State(_state): State<CliState>,
    Json(payload): Json<ShareWorkspaceRequest>,
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

    let port = std::env::var("XAVIER_PORT").unwrap_or_else(|_| "8006".to_string());
    let endpoint = format!("http://localhost:{}", port);

    let token_id = uuid::Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now().timestamp() + 31536000; // 1 year

    let payload_data = serde_json::json!({
        "token_id": token_id.clone(),
        "node_id": identity.node_id.0,
        "endpoint": endpoint,
        "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
        "workspace_id": payload.workspace_id,
        "namespaces": payload.namespaces,
        "expires_at": expires_at,
    });

    let payload_str = match serde_json::to_string(&payload_data) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let signature = identity.sign(payload_str.as_bytes());
    let token_data = serde_json::json!({
        "payload": payload_str,
        "signature": xavier::crypto::hex_encode(&signature),
    });

    let token_json = match serde_json::to_string(&token_data) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let token = xavier::crypto::base64_encode(token_json);

    // Register active consent
    let consent_entry = xavier::mesh::ActiveConsent {
        token_id,
        workspace_id: payload.workspace_id.clone(),
        expires_at: expires_at as u64,
        token: token.clone(),
    };
    if let Err(e) = xavier::mesh::DataConsentManager::register_active_consent(consent_entry) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to register active consent: {}", e) })),
        )
            .into_response();
    }

    Json(ShareWorkspaceResponse { token }).into_response()
}

pub async fn join_workspace_handler(
    State(_state): State<CliState>,
    Json(payload): Json<JoinWorkspaceRequest>,
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

    let decoded_bytes = match xavier::crypto::base64_decode(&payload.token) {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Invalid base64 token" })),
            )
                .into_response();
        }
    };

    let token_data: serde_json::Value = match serde_json::from_slice(&decoded_bytes) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid JSON token: {}", e) })),
            )
                .into_response();
        }
    };

    let payload_str = match token_data.get("payload").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing payload in token" })),
    };

    let signature_hex = match token_data.get("signature").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing signature in token" })),
    };

    let signature_bytes = match xavier::crypto::hex_decode(signature_hex) {
        Ok(b) => b,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Invalid signature hex format" })),
    };

    let inner_payload: serde_json::Value = match serde_json::from_str(payload_str) {
        Ok(v) => v,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Invalid inner payload JSON" })),
    };

    let node_id_str = match inner_payload.get("node_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing node_id in token payload" })),
    };
    
    let endpoint = match inner_payload.get("endpoint").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing endpoint in token payload" })),
    };

    let public_key_hex = match inner_payload.get("public_key_hex").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing public_key_hex in token payload" })),
    };

    let public_key_bytes = match xavier::crypto::hex_decode(&public_key_hex) {
        Ok(b) => b,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Invalid public key hex format" })),
    };

    if !xavier::mesh::node::NodeIdentity::verify(&public_key_bytes, payload_str.as_bytes(), &signature_bytes) {
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({ "error": "Invalid token signature (forgery detected)" }));
    }

    // Determine token_id or use fallback hash of payload_str
    let token_id = inner_payload
        .get("token_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(payload_str.as_bytes());
            format!("hash-{}", &xavier::crypto::hex_encode(hasher.finalize())[..16])
        });

    match xavier::mesh::DataConsentManager::is_token_revoked(&token_id) {
        Ok(true) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Token has been revoked" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Revocation check failed: {}", e) })),
            )
                .into_response();
        }
        _ => {}
    }

    let workspace_id = match inner_payload.get("workspace_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing workspace_id in token payload" })),
    };

    let expires_at = inner_payload.get("expires_at").and_then(|v| v.as_u64()).unwrap_or(0);
    let now = chrono::Utc::now().timestamp() as u64;
    if expires_at < now {
        return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Token has expired" }));
    }

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

    let node_id = NodeId(node_id_str.clone());
    let mut peer = match registry.get_peer(&node_id) {
        Some(existing) => existing.clone(),
        None => PeerInfo {
            node_id: node_id.clone(),
            alias: None,
            endpoint_url: endpoint,
            public_key_hex: public_key_hex.clone(),
            added_at: chrono::Utc::now().timestamp(),
            last_seen_at: None,
            sync_enabled: true,
            is_cloud: false,
            iroh_addr: None,
            shared_workspace_ids: Vec::new(),
            shared_workspace_tokens: std::collections::HashMap::new(),
        },
    };

    if !peer.shared_workspace_ids.contains(&workspace_id) {
        peer.shared_workspace_ids.push(workspace_id.clone());
    }
    peer.shared_workspace_tokens.insert(workspace_id.clone(), payload.token.clone());

    if let Err(e) = registry.add_peer(peer) {
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

    if acl.get_entry(&node_id).is_none() {
        if let Err(e) = acl.set_entry(
            node_id.clone(),
            NodeAclEntry {
                role: Role::Viewer,
                clearance: ClearanceLevel::Unclassified,
                namespaces: None,
                public_key_hex,
                namespace_acl: None,
            },
        ) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    }

    Json(JoinWorkspaceResponse {
        status: "success".to_string(),
        node_id: node_id_str,
        workspace_id,
    }).into_response()
}

pub async fn query_workspace_handler(
    State(state): State<CliState>,
    Json(payload): Json<QueryWorkspaceRequest>,
) -> impl IntoResponse {
    let decoded_bytes = match xavier::crypto::base64_decode(&payload.token) {
        Some(b) => b,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Invalid base64 token" })),
            )
                .into_response();
        }
    };

    let token_data: serde_json::Value = match serde_json::from_slice(&decoded_bytes) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("Invalid JSON token: {}", e) })),
            )
                .into_response();
        }
    };

    let payload_str = match token_data.get("payload").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing payload in token" })),
    };

    let signature_hex = match token_data.get("signature").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing signature in token" })),
    };

    let signature_bytes = match xavier::crypto::hex_decode(signature_hex) {
        Ok(b) => b,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Invalid signature hex format" })),
    };

    let inner_payload: serde_json::Value = match serde_json::from_str(payload_str) {
        Ok(v) => v,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Invalid inner payload JSON" })),
    };

    let public_key_hex = match inner_payload.get("public_key_hex").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing public_key_hex in token payload" })),
    };

    let public_key_bytes = match xavier::crypto::hex_decode(&public_key_hex) {
        Ok(b) => b,
        Err(_) => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Invalid public key hex format" })),
    };

    if !xavier::mesh::node::NodeIdentity::verify(&public_key_bytes, payload_str.as_bytes(), &signature_bytes) {
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({ "error": "Invalid token signature (forgery detected)" }));
    }

    // Determine token_id or use fallback hash of payload_str
    let token_id = inner_payload
        .get("token_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(payload_str.as_bytes());
            format!("hash-{}", &xavier::crypto::hex_encode(hasher.finalize())[..16])
        });

    match xavier::mesh::DataConsentManager::is_token_revoked(&token_id) {
        Ok(true) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Token has been revoked" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Revocation check failed: {}", e) })),
            )
                .into_response();
        }
        _ => {}
    }

    let sender_node_id = inner_payload.get("node_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

    let workspace_id = match inner_payload.get("workspace_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Missing workspace_id in token payload" })),
    };

    let expires_at = inner_payload.get("expires_at").and_then(|v| v.as_u64()).unwrap_or(0);
    let now = chrono::Utc::now().timestamp() as u64;
    if expires_at < now {
        return json_response(StatusCode::BAD_REQUEST, serde_json::json!({ "error": "Token has expired" }));
    }

    // Security check on query input
    let sec_result: xavier::ports::inbound::input_security_port::SecureInputResult = state
        .security
        .process_input(&payload.query)
        .await
        .unwrap_or_else(|_| xavier::ports::inbound::input_security_port::SecureInputResult {
            allowed: false,
            sanitized_input: None,
            original_input: payload.query.clone(),
            detection_confidence: 1.0,
            is_injection: true,
            attack_type: "unknown".to_string(),
        });

    if !sec_result.allowed {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "security_policy_violation",
            })),
        ).into_response();
    }

    let effective_query = sec_result.effective_input();

    let durable_state = match state.store.load_workspace_state(&workspace_id).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to load workspace state: {}", e) })),
            )
                .into_response();
        }
    };

    let allowed_namespaces: Option<Vec<String>> = inner_payload
        .get("namespaces")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let filtered_memories: Vec<&xavier::memory::store::MemoryRecord> = if let Some(ref namespaces) = allowed_namespaces {
        durable_state
            .memories
            .iter()
            .filter(|r| {
                namespaces.iter().any(|pattern| {
                    let record_clean = r.path.trim_end_matches('/');
                    let pattern_clean = pattern.trim_end_matches('/');
                    if record_clean == pattern_clean {
                        true
                    } else {
                        let prefix = format!("{}/", pattern_clean);
                        record_clean.starts_with(&prefix)
                    }
                })
            })
            .collect()
    } else {
        durable_state.memories.iter().collect()
    };

    let docs = std::sync::Arc::new(tokio::sync::RwLock::new(
        filtered_memories
            .iter()
            .map(|r: &&xavier::memory::store::MemoryRecord| r.to_document())
            .collect::<Vec<_>>(),
    ));

    let memory = xavier::memory::qmd_memory::QmdMemory::new_with_workspace(docs, workspace_id.clone());

    let results = match memory.search(effective_query, payload.limit.unwrap_or(10)).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Search failed: {}", e) })),
            )
                .into_response();
        }
    };

    let local_node_id = if let Ok(identity) = xavier::mesh::NodeIdentity::load_or_create() {
        identity.node_id.0
    } else {
        "local".to_string()
    };

    let mut search_results: Vec<serde_json::Value> = results
        .into_iter()
        .map(|document| {
            serde_json::json!({
                "id": document.id,
                "path": document.path,
                "content": document.content,
                "metadata": document.metadata,
                "embedding": document.embedding,
                "source": format!("remote:{}::{}", local_node_id, workspace_id),
                "source_node_id": local_node_id.clone(),
                "source_db_id": workspace_id.clone(),
            })
        })
        .collect();

    let federated = payload.federated.clone().unwrap_or_default();
    let max_hops = federated.max_hops;
    let propagate_to_mesh = federated.propagate_to_mesh;
    let peer_nodes = federated.peer_nodes.clone();

    if max_hops > 0 {
        let next_federated = xavier::memory::schema::FederatedSearchRequest {
            max_hops: max_hops - 1,
            ..federated.clone()
        };

        let mut remote_futures = Vec::new();
        if let Ok(registry) = xavier::mesh::PeerRegistry::load() {
            let peers = registry.list_peers();
            for peer in peers {
                if peer.node_id.0 == sender_node_id {
                    continue;
                }

                if !propagate_to_mesh && !peer_nodes.contains(&peer.node_id.0) {
                    continue;
                }

                for peer_ws_id in &peer.shared_workspace_ids {
                    if let Some(peer_token) = peer.shared_workspace_tokens.get(peer_ws_id) {
                        let client = state.http_client.clone();
                        let url = format!("{}/v1/mesh/workspaces/query", peer.endpoint_url);
                        let query_payload = serde_json::json!({
                            "token": peer_token,
                            "query": payload.query,
                            "limit": payload.limit.unwrap_or(10),
                            "federated": next_federated,
                        });

                        let peer_ws_id = peer_ws_id.clone();
                        let peer_node_id = peer.node_id.0.clone();
                        remote_futures.push(async move {
                            let res = client.post(&url)
                                .json(&query_payload)
                                .timeout(std::time::Duration::from_secs(5))
                                .send()
                                .await;

                            match res {
                                Ok(resp) => {
                                    if resp.status().is_success() {
                                        if let Ok(body) = resp.json::<serde_json::Value>().await {
                                            if let Some(results_arr) = body.get("results").and_then(|v| v.as_array()) {
                                                let mut remote_docs = Vec::new();
                                                for r in results_arr {
                                                    let mut r_clone = r.clone();
                                                    if let Some(obj) = r_clone.as_object_mut() {
                                                        obj.insert("source".to_string(), serde_json::json!(format!("remote:{}::{}", peer_node_id, peer_ws_id)));
                                                        if obj.get("source_node_id").is_none() {
                                                            obj.insert("source_node_id".to_string(), serde_json::json!(peer_node_id.clone()));
                                                        }
                                                        if obj.get("source_db_id").is_none() {
                                                            obj.insert("source_db_id".to_string(), serde_json::json!(peer_ws_id.clone()));
                                                        }
                                                    }
                                                    remote_docs.push(r_clone);
                                                }
                                                return Some(remote_docs);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to query remote workspace {} on peer {}: {}", peer_ws_id, peer_node_id, e);
                                }
                            }
                            None
                        });
                    }
                }
            }
        }

        let remote_results = futures_util::future::join_all(remote_futures).await;
        for mut remote_list in remote_results.into_iter().flatten() {
            search_results.append(&mut remote_list);
        }
    }

    Json(serde_json::json!({
        "status": "ok",
        "query": payload.query,
        "count": search_results.len(),
        "results": search_results,
        "workspace_id": workspace_id,
    })).into_response()
}

pub async fn v1_mesh_status_handler() -> impl IntoResponse {
    // License check
    let settings = xavier::settings::XavierSettings::current();
    if let Err(e) = xavier::security::license::require_mesh_license(&settings) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response();
    }

    Json(MeshMaturityReport::default()).into_response()
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
