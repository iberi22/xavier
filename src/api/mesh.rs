//! Mesh peer management API endpoints
//!
//! - GET  /v1/mesh/peers      → list registered peers
//! - POST /v1/mesh/peers      → add a new peer
//! - DELETE /v1/mesh/peers/:id → remove a peer
//! - GET  /v1/mesh/health     → mesh connectivity status
//! - POST /v1/mesh/heartbeat  → update peer last_seen_at

use crate::mesh::node::NodeId;
use crate::mesh::peer::{PeerInfo, PeerRegistry};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MeshState {
    pub registry: Arc<RwLock<PeerRegistry>>,
}

#[derive(Serialize)]
pub struct PeersResponse {
    pub peers: Vec<PeerView>,
    pub total: usize,
    pub healthy: usize,
}

#[derive(Serialize)]
pub struct PeerView {
    pub node_id: String,
    pub alias: Option<String>,
    pub endpoint_url: String,
    pub has_iroh: bool,
    pub last_seen_at: Option<i64>,
    pub sync_enabled: bool,
    pub is_cloud: bool,
    pub healthy: bool,
    pub valid: bool,
}

impl From<&PeerInfo> for PeerView {
    fn from(p: &PeerInfo) -> Self {
        Self {
            node_id: p.node_id.0.clone(),
            alias: p.alias.clone(),
            endpoint_url: p.endpoint_url.clone(),
            has_iroh: p.iroh_addr.is_some(),
            last_seen_at: p.last_seen_at,
            sync_enabled: p.sync_enabled,
            is_cloud: p.is_cloud,
            healthy: p.is_healthy(),
            valid: p.is_valid(),
        }
    }
}

#[derive(Deserialize)]
pub struct AddPeerRequest {
    pub node_id: String,
    pub endpoint_url: String,
    pub alias: Option<String>,
}

#[derive(Deserialize)]
pub struct HeartbeatRequest {
    pub node_id: String,
    pub version: Option<String>,
}

#[derive(Serialize)]
pub struct MeshHealthResponse {
    pub status: String,
    pub total_peers: usize,
    pub healthy_peers: usize,
    pub connected_peers: usize,
}

pub fn mesh_routes(state: Arc<RwLock<MeshState>>) -> Router {
    Router::new()
        .route("/peers", get(list_peers).post(add_peer))
        .route("/peers/{node_id}", delete(remove_peer))
        .route("/health", get(mesh_health))
        .route("/heartbeat", post(heartbeat))
        .with_state(state)
}

async fn list_peers(
    State(state): State<Arc<RwLock<MeshState>>>,
) -> Json<PeersResponse> {
    let state = state.read().await;
    let registry = state.registry.read().await;
    let peers: Vec<PeerView> = registry.all_peers().iter().map(PeerView::from).collect();
    let healthy = peers.iter().filter(|p| p.healthy).count();
    let total = peers.len();
    Json(PeersResponse { peers, total, healthy })
}

async fn add_peer(
    State(state): State<Arc<RwLock<MeshState>>>,
    Json(req): Json<AddPeerRequest>,
) -> Result<Json<PeerView>, (StatusCode, String)> {
    if req.node_id.is_empty() || req.endpoint_url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "node_id and endpoint_url required".into()));
    }
    let peer = PeerInfo {
        node_id: NodeId(req.node_id.clone()),
        alias: req.alias,
        endpoint_url: req.endpoint_url,
        public_key_hex: String::new(),
        added_at: chrono::Utc::now().timestamp(),
        last_seen_at: None,
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: None,
        shared_workspace_ids: vec![],
        shared_workspace_tokens: Default::default(),
    };
    let state = state.read().await;
    let mut registry = state.registry.write().await;
    registry.add_peer(peer.clone());
    registry.save().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Save: {e}")))?;
    Ok(Json(PeerView::from(&peer)))
}

async fn remove_peer(
    State(state): State<Arc<RwLock<MeshState>>>,
    Path(node_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let state = state.read().await;
    let mut registry = state.registry.write().await;
    let removed = registry.remove_peer(&NodeId(node_id.clone()));
    if !removed {
        return Err((StatusCode::NOT_FOUND, format!("Peer {node_id} not found")));
    }
    registry.save().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Save: {e}")))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mesh_health(
    State(state): State<Arc<RwLock<MeshState>>>,
) -> Json<MeshHealthResponse> {
    let state = state.read().await;
    let registry = state.registry.read().await;
    let all = registry.all_peers();
    let total = all.len();
    let healthy = all.iter().filter(|p| p.is_healthy()).count();
    let connected = all.iter().filter(|p| p.is_valid() && p.is_healthy()).count();
    let status = if connected > 0 {
        "healthy"
    } else if total > 0 {
        "degraded"
    } else {
        "no_peers"
    };
    Json(MeshHealthResponse {
        status: status.into(),
        total_peers: total,
        healthy_peers: healthy,
        connected_peers: connected,
    })
}

async fn heartbeat(
    State(state): State<Arc<RwLock<MeshState>>>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if req.node_id.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "node_id required".into()));
    }
    let state = state.read().await;
    let mut registry = state.registry.write().await;
    if let Some(peer) = registry.get_peer_mut(&NodeId(req.node_id.clone())) {
        peer.last_seen_at = Some(chrono::Utc::now().timestamp());
        registry.save().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Save: {e}")))?;
        Ok(StatusCode::OK)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Peer {} not found", req.node_id)))
    }
}
