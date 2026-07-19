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
use xavier::memory::schema::ClearanceLevel;
use xavier::mesh::{
    acl::{MeshAcl, NodeAclEntry},
    NodeId, NodeIdentity, PeerInfo, PeerRegistry, MeshMaturityReport,
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

#[derive(Debug, Deserialize)]
pub struct ShareWorkspaceRequest {
    pub workspace_id: String,
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

#[derive(Debug, Deserialize)]
pub struct QueryWorkspaceRequest {
    pub token: String,
    pub query: String,
    pub limit: Option<usize>,
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

    let payload_data = serde_json::json!({
        "node_id": identity.node_id.0,
        "endpoint": endpoint,
        "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
        "workspace_id": payload.workspace_id,
        "expires_at": chrono::Utc::now().timestamp() + 31536000, // 1 year
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

    let docs = std::sync::Arc::new(tokio::sync::RwLock::new(
        durable_state
            .memories
            .iter()
            .map(|r: &xavier::memory::store::MemoryRecord| r.to_document())
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

    let search_results: Vec<serde_json::Value> = results
        .into_iter()
        .map(|document| {
            serde_json::json!({
                "id": document.id,
                "content": document.content,
                "embedding": document.embedding,
            })
        })
        .collect();

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
