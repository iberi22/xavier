use axum::extract::{Json, State};
use axum::response::IntoResponse;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use xavier::mesh::{PeerInfo, PeerRegistry, NodeId};
use xavier::settings::XavierSettings;

// Import our handlers from the binary crate if possible, or define mock tests.
// Since CLI handlers are in the main binary target of `xavier`, let's check if they can be verified directly.
// Let's test the PeerRegistry serialization/deserialization with our extended fields!

#[test]
fn test_peer_registry_with_workspace_federation_fields() {
    let dir = tempdir().unwrap();
    let storage_path = dir.path().join("peers_federation.json");

    let mut registry = PeerRegistry::load_from(storage_path.clone()).unwrap();

    let node_id = NodeId("xv1-federated-peer".to_string());
    let mut peer = PeerInfo {
        node_id: node_id.clone(),
        alias: Some("Federated Peer".to_string()),
        endpoint_url: "http://localhost:8006".to_string(),
        public_key_hex: "aabbccdd".to_string(),
        added_at: 1000,
        last_seen_at: None,
        sync_enabled: true,
        is_cloud: false,
        iroh_addr: None,
        shared_workspace_ids: Vec::new(),
        shared_workspace_tokens: HashMap::new(),
    };

    peer.shared_workspace_ids.push("workspace-alpha".to_string());
    peer.shared_workspace_tokens.insert("workspace-alpha".to_string(), "mock-token-xyz".to_string());

    registry.add_peer(peer).unwrap();
    assert_eq!(registry.list_peers().len(), 1);

    // Reload registry from file
    let reloaded = PeerRegistry::load_from(storage_path).unwrap();
    let loaded_peer = reloaded.get_peer(&node_id).unwrap();

    assert_eq!(loaded_peer.shared_workspace_ids.len(), 1);
    assert_eq!(loaded_peer.shared_workspace_ids[0], "workspace-alpha");
    assert_eq!(loaded_peer.shared_workspace_tokens.get("workspace-alpha").unwrap(), "mock-token-xyz");
}

#[tokio::test]
async fn test_workspace_sharing_token_roundtrip() {
    let payload = serde_json::json!({
        "node_id": "xv1-localnode",
        "endpoint": "http://localhost:8006",
        "public_key_hex": "123456",
        "workspace_id": "default-workspace",
        "expires_at": chrono::Utc::now().timestamp() + 3600,
    });

    let token_json = serde_json::to_string(&payload).unwrap();
    let token = xavier::crypto::base64_encode(token_json);

    let decoded_bytes = xavier::crypto::base64_decode(&token).unwrap();
    let token_data: serde_json::Value = serde_json::from_slice(&decoded_bytes).unwrap();

    assert_eq!(token_data["workspace_id"], "default-workspace");
    assert_eq!(token_data["node_id"], "xv1-localnode");
}
