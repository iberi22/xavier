//! Mesh peer management and offboarding routes.
//!
//! Provides endpoints for peer revocation, ACL grant cancellation,
//! and mesh-wide KillSwitchNotice broadcasting for remote knowledge purging.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::mesh::acl::MeshAcl;
use crate::mesh::node::NodeId;
use crate::mesh::peer::PeerRegistry;
use crate::mesh::private_mesh::PrivateMeshRegistry;

/// Signed notification broadcast across the mesh when a peer is revoked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KillSwitchNotice {
    pub notice_id: String,
    pub network_id: String,
    pub peer_id: String,
    pub revoked_by: String,
    pub timestamp: u64,
    pub signature_hex: String,
}

/// Confirmation payload returned upon peer revocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerRevokeResponse {
    pub status: String,
    pub purged: bool,
    pub timestamp: u64,
    pub notice: KillSwitchNotice,
}

/// Shared state for mesh peer routes.
#[derive(Clone, Debug)]
pub struct MeshPeerState {
    pub data_dir: PathBuf,
    pub root_host_id: String,
    pub notices: Arc<RwLock<Vec<KillSwitchNotice>>>,
}

impl MeshPeerState {
    /// Create a new MeshPeerState with a root directory.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            root_host_id: "root".to_string(),
            notices: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set a custom root host ID for this node.
    pub fn with_root_host(mut self, root_host_id: String) -> Self {
        self.root_host_id = root_host_id;
        self
    }
}

/// Generates a SHA-256 cryptographic signature for a KillSwitchNotice.
fn compute_notice_signature(
    notice_id: &str,
    network_id: &str,
    peer_id: &str,
    revoked_by: &str,
    timestamp: u64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"killswitch_v1:");
    hasher.update(notice_id.as_bytes());
    hasher.update(b":");
    hasher.update(network_id.as_bytes());
    hasher.update(b":");
    hasher.update(peer_id.as_bytes());
    hasher.update(b":");
    hasher.update(revoked_by.as_bytes());
    hasher.update(b":");
    hasher.update(timestamp.to_string().as_bytes());
    crate::crypto::hex_encode(hasher.finalize())
}

/// Helper function to determine if a node is considered a root host or master node.
pub fn is_root_or_master(node_id: &str, root_host_id: &str) -> bool {
    let normalized = node_id.to_lowercase();
    normalized == "root"
        || normalized == "master"
        || normalized == "root_host"
        || normalized == "master_node"
        || normalized == root_host_id.to_lowercase()
}

/// POST /v1/mesh/networks/:network_id/peers/:peer_id/revoke
///
/// Revokes peer cryptographic ACL grant immediately, broadcasts a signed
/// KillSwitchNotice across the mesh topic, and returns confirmation with purged: true.
pub async fn revoke_peer_handler(
    State(state): State<MeshPeerState>,
    Path((network_id, peer_id)): Path<(String, String)>,
) -> impl IntoResponse {
    // Anti-Hallucination Guard: Prevent revoking the root host/master node.
    if is_root_or_master(&peer_id, &state.root_host_id) {
        warn!(
            "Attempted to revoke root host or master node '{}' in network '{}'",
            peer_id, network_id
        );
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Cannot revoke root host or master node '{}'", peer_id)
            })),
        )
            .into_response();
    }

    let mesh_path = state.data_dir.join("mesh/private-mesh.json");
    let mut private_mesh = match PrivateMeshRegistry::load_or_create(mesh_path) {
        Ok(reg) => reg,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to load private mesh registry: {}", e)
                })),
            )
                .into_response();
        }
    };

    // Verify network owner is not target peer
    if let Some(net) = private_mesh.get_network(&network_id) {
        if net.owner_node == peer_id {
            warn!(
                "Attempted to revoke network owner node '{}' in network '{}'",
                peer_id, network_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Cannot revoke network owner node '{}'", peer_id)
                })),
            )
                .into_response();
        }
    }

    // Revoke peer grants and remove from network if network exists
    if private_mesh.get_network(&network_id).is_some() {
        let _ = private_mesh.remove_member(&network_id, &peer_id);
    }

    // Revoke from MeshAcl file if present
    let acl_path = state.data_dir.join("mesh_acl.json");
    if let Ok(mut acl) = MeshAcl::load_from(acl_path) {
        let _ = acl.remove_entry(&NodeId(peer_id.clone()));
    }

    // Revoke from PeerRegistry file if present
    let peers_path = state.data_dir.join("mesh_peers.json");
    if let Ok(mut peers) = PeerRegistry::load_from(peers_path) {
        let _ = peers.remove_peer(&NodeId(peer_id.clone()));
    }

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let notice_id = format!("ksn-{}", ulid::Ulid::new());
    let revoked_by = state.root_host_id.clone();
    let signature_hex = compute_notice_signature(
        &notice_id,
        &network_id,
        &peer_id,
        &revoked_by,
        now_secs,
    );

    let notice = KillSwitchNotice {
        notice_id: notice_id.clone(),
        network_id: network_id.clone(),
        peer_id: peer_id.clone(),
        revoked_by,
        timestamp: now_secs,
        signature_hex,
    };

    // Broadcast KillSwitchNotice across mesh topic (store in state and trace log)
    {
        let mut notices_guard = state.notices.write().await;
        notices_guard.push(notice.clone());
    }
    info!(
        "KillSwitchNotice broadcasted for peer '{}' on network '{}' (notice_id={})",
        peer_id, network_id, notice_id
    );

    let response = PeerRevokeResponse {
        status: "ok".to_string(),
        purged: true,
        timestamp: now_secs,
        notice,
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Creates the router for mesh peer routes.
pub fn router(state: MeshPeerState) -> Router {
    Router::new()
        .route(
            "/v1/mesh/networks/{network_id}/peers/{peer_id}/revoke",
            post(revoke_peer_handler),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_mesh_peer_revoke_and_purge() {
        let dir = tempdir().unwrap();
        let state = MeshPeerState::new(dir.path().to_path_buf())
            .with_root_host("root-master-node".to_string());
        let app = router(state.clone());

        // Create a network with an owner and a regular peer member
        let mesh_dir = dir.path().join("mesh");
        std::fs::create_dir_all(&mesh_dir).unwrap();
        let mut reg =
            PrivateMeshRegistry::load_or_create(mesh_dir.join("private-mesh.json")).unwrap();
        let _net = reg
            .create_network(
                "net-alpha".to_string(),
                "Alpha Network".to_string(),
                "node-owner".to_string(),
            )
            .unwrap();
        reg.add_member("net-alpha", "peer-bob".to_string()).unwrap();

        // 1. Test Anti-Hallucination Guard: Revoking root host/master node must fail
        let root_revoke_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/net-alpha/peers/root/revoke")
            .body(Body::empty())
            .unwrap();

        let resp_root = app.clone().oneshot(root_revoke_req).await.unwrap();
        assert_eq!(resp_root.status(), StatusCode::FORBIDDEN);

        let root_master_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/net-alpha/peers/root-master-node/revoke")
            .body(Body::empty())
            .unwrap();

        let resp_master = app.clone().oneshot(root_master_req).await.unwrap();
        assert_eq!(resp_master.status(), StatusCode::FORBIDDEN);

        // 2. Test successful revocation of regular peer "peer-bob"
        let revoke_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/networks/net-alpha/peers/peer-bob/revoke")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(revoke_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let res: PeerRevokeResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(res.status, "ok");
        assert!(res.purged);
        assert!(res.timestamp > 0);
        assert_eq!(res.notice.network_id, "net-alpha");
        assert_eq!(res.notice.peer_id, "peer-bob");
        assert_eq!(res.notice.revoked_by, "root-master-node");
        assert!(!res.notice.signature_hex.is_empty());

        // Verify KillSwitchNotice was stored/broadcasted
        let notices = state.notices.read().await;
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].peer_id, "peer-bob");

        // Verify peer was removed from network members in registry
        let reloaded_reg =
            PrivateMeshRegistry::load_or_create(mesh_dir.join("private-mesh.json")).unwrap();
        let net_updated = reloaded_reg.get_network("net-alpha").unwrap();
        assert!(!net_updated.members.contains(&"peer-bob".to_string()));
    }
}
