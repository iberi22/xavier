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
    let identity = xavier::mesh::node::NodeIdentity::generate();
    let payload_data = serde_json::json!({
        "node_id": "xv1-localnode",
        "endpoint": "http://localhost:8006",
        "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
        "workspace_id": "default-workspace",
        "expires_at": chrono::Utc::now().timestamp() + 3600,
    });

    let payload_str = serde_json::to_string(&payload_data).unwrap();
    let signature = identity.sign(payload_str.as_bytes());

    let token_data = serde_json::json!({
        "payload": payload_str,
        "signature": xavier::crypto::hex_encode(&signature),
    });

    let token_json = serde_json::to_string(&token_data).unwrap();
    let token = xavier::crypto::base64_encode(token_json);

    let decoded_bytes = xavier::crypto::base64_decode(&token).unwrap();
    let decoded_token_data: serde_json::Value = serde_json::from_slice(&decoded_bytes).unwrap();
    let decoded_payload_str = decoded_token_data["payload"].as_str().unwrap();
    let decoded_payload: serde_json::Value = serde_json::from_str(decoded_payload_str).unwrap();

    assert_eq!(decoded_payload["workspace_id"], "default-workspace");
    assert_eq!(decoded_payload["node_id"], "xv1-localnode");
}
